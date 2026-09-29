//! TLS：由 SslConfig 解析 rustls 配置，含跳过证书校验的 NoVerifier。

use std::sync::Arc;

use rumqttc::TlsConfiguration;
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};

use crate::model::{
    ConnectionConfig,
    SslConfig,
};// ─── 引擎事件 ────────────────────────────────────────────────────────────────



/// SSL 配置非默认时构建自定义 rustls 配置；返回 None 表示沿用 rumqttc 默认 TLS。
/// 文件读取/解析失败在此返回 Err，由 connect 的错误通道上报为连接失败事件。
pub(super) fn resolve_tls_config(cfg: &ConnectionConfig) -> Result<Option<TlsConfiguration>, String> {
    if !cfg.transport.is_tls() || cfg.ssl.is_default() {
        return Ok(None);
    }
    Ok(Some(TlsConfiguration::Rustls(build_rustls_config(&cfg.ssl)?)))
}

/// 按 SslConfig 构造 rustls ClientConfig：
/// - `ignore_ca`：跳过服务器证书链/主机名校验；
/// - `ca_file`：在系统根证书之外追加自定义 CA；
/// - `client_cert_file` + `client_key_file`：双向认证。
pub(super) fn build_rustls_config(ssl: &SslConfig) -> Result<Arc<rustls::ClientConfig>, String> {
    // 只填证书或只填私钥必然握手失败，提前给出明确错误
    if ssl.client_cert_file.is_empty() != ssl.client_key_file.is_empty() {
        return Err("客户端证书与客户端密钥必须同时配置".into());
    }
    let provider = ensure_crypto_provider();
    let builder = rustls::ClientConfig::builder();
    let builder = if ssl.ignore_ca {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier {
                provider: provider.clone(),
            }))
    } else {
        let mut roots = rustls::RootCertStore::empty();
        let native = rustls_native_certs::load_native_certs();
        if native.certs.is_empty() && ssl.ca_file.is_empty() {
            let detail = native
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(format!("加载系统根证书失败: {detail}"));
        }
        // 个别系统证书加载失败不阻断连接：自定义 CA 与其余根证书仍参与校验
        for cert in native.certs {
            let _ = roots.add(cert);
        }
        if !ssl.ca_file.is_empty() {
            let pem = std::fs::read(&ssl.ca_file)
                .map_err(|e| format!("读取 CA 文件 {} 失败: {e}", ssl.ca_file))?;
            let mut added = 0usize;
            for cert in CertificateDer::pem_slice_iter(&pem) {
                let cert = cert.map_err(|e| format!("解析 CA 文件 {} 失败: {e}", ssl.ca_file))?;
                roots
                    .add(cert)
                    .map_err(|e| format!("CA 文件 {} 中存在无效证书: {e}", ssl.ca_file))?;
                added += 1;
            }
            if added == 0 {
                return Err(format!("CA 文件 {} 中没有找到任何证书", ssl.ca_file));
            }
        }
        builder.with_root_certificates(roots)
    };

    let config = if ssl.client_cert_file.is_empty() {
        builder.with_no_client_auth()
    } else {
        let chain_pem = std::fs::read(&ssl.client_cert_file).map_err(|e| {
            format!("读取客户端证书文件 {} 失败: {e}", ssl.client_cert_file)
        })?;
        let key_pem = std::fs::read(&ssl.client_key_file)
            .map_err(|e| format!("读取客户端私钥文件 {} 失败: {e}", ssl.client_key_file))?;
        let certs = CertificateDer::pem_slice_iter(&chain_pem)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("解析客户端证书文件 {} 失败: {e}", ssl.client_cert_file))?;
        if certs.is_empty() {
            return Err(format!(
                "客户端证书文件 {} 中没有找到证书",
                ssl.client_cert_file
            ));
        }
        let key = PrivateKeyDer::from_pem_slice(&key_pem)
            .map_err(|e| format!("解析客户端私钥文件 {} 失败: {e}", ssl.client_key_file))?;
        builder
            .with_client_auth_cert(certs, key)
            .map_err(|e| format!("加载客户端证书/私钥失败: {e}"))?
    };
    Ok(Arc::new(config))
}

/// 取得（必要时安装）进程级 CryptoProvider。
/// 工作区同时启用 ring 与 aws-lc-rs 特性时 rustls 无法自动推断默认 provider，
/// 不显式安装会让 `ClientConfig::builder()` panic。
pub(super) fn ensure_crypto_provider() -> Arc<rustls::crypto::CryptoProvider> {
    if let Some(provider) = rustls::crypto::CryptoProvider::get_default() {
        return Arc::clone(provider);
    }
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    // 输掉安装竞争时直接采用已安装的那个
    match provider.clone().install_default() {
        Ok(()) => Arc::new(provider),
        Err(installed) => installed,
    }
}

/// 「忽略 CA 校验」用：跳过服务器证书链与主机名校验，
/// 握手签名仍用 provider 校验（与 rustls 官方示例做法一致）。
pub(super) struct NoVerifier {
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl std::fmt::Debug for NoVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoVerifier").finish_non_exhaustive()
    }
}

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// 去掉空白字符串，避免表单存了空串也写进报文属性。
pub(super) fn non_blank(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.trim().is_empty())
}
