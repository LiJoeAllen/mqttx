//! 新建/编辑连接的表单对话框内容。

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    h_flex, notification::Notification, v_flex, ActiveTheme as _, Icon, Sizable as _,
    StyledExt as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use crate::ui::IconName;
use crate::ui::i18n;
use gpui_kit::{
    div, percentage, px, App, AppContext as _, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window,
};

use crate::model::{ConnectionConfig, LastWill, ProtocolVersion, SslConfig, TransportKind};
use crate::mqtt::MqttEngine;
use crate::ui::widgets::{field, field_ex, make_select, OptionDelegate};

fn protocol_labels() -> [&'static str; 2] { ["MQTT 5.0", "MQTT 3.1.1"] }
const TRANSPORTS: [&str; 4] = ["TCP", "TLS", "WebSocket", "WSS"]; // 语言无关
const QOS: [&str; 3] = ["QoS 0", "QoS 1", "QoS 2"]; // 语言无关
/// 分组下拉的首项：选择它表示不归属任何分组。
fn no_group_label() -> &'static str { i18n::t("form.no_group") }

pub struct ConnectionForm {
    engine: Arc<MqttEngine>,
    editing_id: Option<String>,
    created_at: i64,
    /// 已有连接的分组列表（去重），用于分组下拉
    groups: Vec<String>,

    name: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    path: Entity<InputState>,
    client_id: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    keep_alive: Entity<InputState>,
    session_expiry: Entity<InputState>,
    receive_max: Entity<InputState>,
    max_packet: Entity<InputState>,
    topic_alias: Entity<InputState>,
    conn_timeout: Entity<InputState>,
    max_reconnect: Entity<InputState>,

    will_topic: Entity<InputState>,
    will_payload: Entity<InputState>,
    will_content_type: Entity<InputState>,
    will_response_topic: Entity<InputState>,

    // SSL/TLS 区块（transport 为 TLS/WSS 时显示）
    ssl_ca: Entity<InputState>,
    ssl_client_cert: Entity<InputState>,
    ssl_client_key: Entity<InputState>,

    // 新分组名输入；非空时优先于下拉选择
    group_new: Entity<InputState>,

    protocol: Entity<SelectState<OptionDelegate>>,
    transport: Entity<SelectState<OptionDelegate>>,
    will_qos: Entity<SelectState<OptionDelegate>>,
    group_select: Entity<SelectState<OptionDelegate>>,

    clean_start: bool,
    auto_resubscribe: bool,
    auto_reconnect: bool,
    auto_connect: bool,
    will_enabled: bool,
    will_retain: bool,
    ssl_ignore_ca: bool,

    // 按需显示：高级分组默认折叠（编辑已配置内容时在 new() 里默认展开）
    show_auth: bool,
    show_mqtt5: bool,
    show_will: bool,
    show_ssl: bool,

    // build() 校验失败时的字段级错误（字段 key → 提示文案），
    // render 时映射成红字；build 只拿 &self，故用 RefCell 内部可变
    field_errors: RefCell<HashMap<&'static str, String>>,

    testing: bool,
    _subs: Vec<Subscription>,
}

impl ConnectionForm {
    /// 对话框 footer 的测试按钮需要读取测试进行中的状态。
    pub fn is_testing(&self) -> bool {
        self.testing
    }
}

fn input(
    value: &str,
    placeholder: &str,
    window: &mut Window,
    cx: &mut Context<ConnectionForm>,
) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value)
    })
}

fn optional_input(
    value: Option<u32>,
    placeholder: &str,
    window: &mut Window,
    cx: &mut Context<ConnectionForm>,
) -> Entity<InputState> {
    cx.new(|cx| {
        let s = InputState::new(window, cx).placeholder(placeholder);
        match value {
            Some(v) => s.default_value(v.to_string()),
            None => s,
        }
    })
}

impl ConnectionForm {
    pub fn new(
        engine: Arc<MqttEngine>,
        edit: Option<ConnectionConfig>,
        groups: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let c = edit.clone().unwrap_or_default();
        let editing_id = edit.as_ref().map(|e| e.id.clone());
        let created_at = edit.as_ref().map(|e| e.created_at).unwrap_or_else(|| {
            chrono::Local::now().timestamp()
        });

        let name = input(&c.name, i18n::t("form.name_ph"), window, cx);
        let host = input(&c.host, "broker.example.com", window, cx);
        let port = input(&c.port.to_string(), i18n::t(i18n::t("form.port")), window, cx);
        let path = input(&c.path, "/mqtt", window, cx);
        let client_id = input(&c.client_id, i18n::t("form.client_id"), window, cx);
        let username = input(&c.username, i18n::t("form.username_ph"), window, cx);
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(i18n::t("form.password_ph"))
                .default_value(&c.password)
                .masked(true)
        });
        let keep_alive = input(&c.keep_alive.to_string(), "60", window, cx);
        let conn_timeout = input(
            &c.connection_timeout_secs.to_string(),
            i18n::t("form.timeout_ph"),
            window,
            cx,
        );
        let max_reconnect = input(
            &c.max_reconnect_times.to_string(),
            i18n::t("form.max_reconnects_ph"),
            window,
            cx,
        );
        let session_expiry = input(
            &if c.session_expiry_interval > 0 {
                c.session_expiry_interval.to_string()
            } else {
                String::new()
            },
            i18n::t("form.session_expiry_ph"),
            window,
            cx,
        );
        let receive_max = optional_input(c.receive_maximum.map(u32::from), i18n::t("form.receive_max_ph"), window, cx);
        let max_packet = optional_input(c.maximum_packet_size, i18n::t("form.max_packet"), window, cx);
        let topic_alias = optional_input(c.topic_alias_maximum.map(u32::from), i18n::t("form.topic_alias_max"), window, cx);

        let ssl_ca = input(&c.ssl.ca_file, i18n::t("ssl.ca_ph"), window, cx);
        let ssl_client_cert = input(&c.ssl.client_cert_file, i18n::t("ssl.cert_ph"), window, cx);
        let ssl_client_key = input(&c.ssl.client_key_file, i18n::t("ssl.key_ph"), window, cx);

        let group_new = input("", i18n::t("form.new_group_hint"), window, cx);

        let (will_topic, will_payload, will_enabled, will_retain, will_qos_idx, will_content_type, will_response_topic) =
            match &c.last_will {
                Some(w) => (
                    input(&w.topic, i18n::t("form.will_topic"), window, cx),
                    input(&w.payload, i18n::t("form.will_payload_ph"), window, cx),
                    true,
                    w.retain,
                    w.qos as usize,
                    input(w.content_type.as_deref().unwrap_or(""), i18n::t("form.content_type_ph"), window, cx),
                    input(w.response_topic.as_deref().unwrap_or(""), i18n::t("form.response_topic_ph"), window, cx),
                ),
                None => (
                    input("", i18n::t("form.will_topic"), window, cx),
                    input("", i18n::t("form.will_payload_ph"), window, cx),
                    false,
                    false,
                    0,
                    input("", i18n::t("form.content_type_ph"), window, cx),
                    input("", i18n::t("form.response_topic_ph"), window, cx),
                ),
            };

        let protocol = make_select(
            &protocol_labels(),
            if c.protocol.is_v5() { 0 } else { 1 },
            window,
            cx,
        );
        let transport_idx = TransportKind::ALL
            .iter()
            .position(|t| *t == c.transport)
            .unwrap_or(0);
        let transport = make_select(&TRANSPORTS, transport_idx, window, cx);
        let will_qos = make_select(&QOS, will_qos_idx, window, cx);
        // 分组下拉：首项固定为「无分组」，其后是已有分组
        let mut group_labels = vec![no_group_label()];
        group_labels.extend(groups.iter().map(|s| s.as_str()));
        let group_idx = c
            .group
            .as_deref()
            .and_then(|g| {
                // 存储值可能带首尾空白，而下拉列表在上游已 trim，
                // 直接相等比较会匹配不上 → 回填成「无分组」，保存即丢分组
                let g = g.trim();
                groups.iter().position(|x| x == g)
            })
            .map(|i| i + 1)
            .unwrap_or(0);
        let group_select = make_select(&group_labels, group_idx, window, cx);

        // 切换传输方式时，把端口同步成该方式的默认端口，并刷新 WS 路径的按需显示。
        // gpui 列表点击任意项（含当前项）都会发 Confirm，若不比对索引，
        // 打开下拉点一下当前传输就会把自定义端口静默改回默认值。
        // 记录"上一个传输方式"而非索引：端口只在仍是上一个方式的默认端口时才跟随，
        // 与全部默认端口比较会把用户恰好手输的 8084/8883 等值误判为默认值而覆盖
        let last_transport = std::cell::Cell::new(TransportKind::ALL[transport_idx]);
        let port_for_sub = port.clone();
        let sub_transport = cx.subscribe_in(
            &transport,
            window,
            move |_this, state, event, window, cx| {
                if matches!(event, SelectEvent::Confirm(Some(_)))
                    && let Some(idx) = state.read(cx).selected_value().copied()
                    && let Some(kind) = TransportKind::ALL.get(idx).copied()
                {
                    // 未变化：视为重新点选当前项，不覆盖用户输入的端口
                    if last_transport.get() == kind {
                        return;
                    }
                    // WSS:9443 → TCP → WSS 的往返不会丢失 9443
                    let is_prev_default = port_for_sub.read(cx).value().trim().parse::<u16>().ok()
                        == Some(last_transport.get().default_port());
                    last_transport.set(kind);
                    if is_prev_default {
                        port_for_sub.update(cx, |p, cx| {
                            p.set_value(kind.default_port().to_string(), window, cx);
                        });
                    }
                    cx.notify();
                }
            },
        );
        // 切换协议时刷新 MQTT 5 属性分组的按需显示。
        let sub_protocol = cx.subscribe_in(
            &protocol,
            window,
            move |_this, _state, event, _window, cx| {
                if matches!(event, SelectEvent::Confirm(Some(_))) {
                    cx.notify();
                }
            },
        );

        // 编辑已配置 SSL/会话过期等高级项的连接时默认展开，避免配置不可见
        let ssl_configured = !c.ssl.ca_file.trim().is_empty()
            || !c.ssl.client_cert_file.trim().is_empty()
            || !c.ssl.client_key_file.trim().is_empty()
            || c.ssl.ignore_ca;
        let mqtt5_configured = c.session_expiry_interval > 0
            || c.receive_maximum.is_some()
            || c.maximum_packet_size.is_some()
            || c.topic_alias_maximum.is_some();

        // 任一被校验字段被编辑时清空字段错误：错误只在保存/测试时登记，若不随输入
        // 清除，用户改好值后红字仍然挂着，无从判断当前输入是否已合法
        let err_watch: [Entity<InputState>; 16] = [
            name.clone(),
            host.clone(),
            port.clone(),
            path.clone(),
            client_id.clone(),
            keep_alive.clone(),
            conn_timeout.clone(),
            max_reconnect.clone(),
            session_expiry.clone(),
            receive_max.clone(),
            max_packet.clone(),
            topic_alias.clone(),
            ssl_ca.clone(),
            ssl_client_cert.clone(),
            ssl_client_key.clone(),
            will_topic.clone(),
        ];
        let mut err_subs = Vec::with_capacity(err_watch.len());
        for w in err_watch {
            err_subs.push(cx.subscribe_in(
                &w,
                window,
                move |this, _, ev: &InputEvent, _, cx| {
                    if matches!(ev, InputEvent::Change)
                        && !this.field_errors.borrow().is_empty()
                    {
                        this.field_errors.borrow_mut().clear();
                        cx.notify();
                    }
                },
            ));
        }

        Self {
            engine,
            editing_id,
            created_at,
            groups,
            name,
            host,
            port,
            path,
            client_id,
            username,
            password,
            keep_alive,
            conn_timeout,
            max_reconnect,
            session_expiry,
            receive_max,
            max_packet,
            topic_alias,
            ssl_ca,
            ssl_client_cert,
            ssl_client_key,
            group_new,
            will_topic,
            will_payload,
            will_content_type,
            will_response_topic,
            protocol,
            transport,
            will_qos,
            group_select,
            clean_start: c.clean_start,
            auto_resubscribe: c.auto_resubscribe,
            auto_reconnect: c.auto_reconnect,
            auto_connect: c.auto_connect,
            will_enabled,
            will_retain,
            ssl_ignore_ca: c.ssl.ignore_ca,
            show_auth: true,
            show_mqtt5: mqtt5_configured,
            show_will: false,
            show_ssl: ssl_configured,
            field_errors: RefCell::default(),
            testing: false,
            _subs: {
                let mut subs = vec![sub_transport, sub_protocol];
                subs.extend(err_subs);
                subs
            },
        }
    }

    fn val(&self, e: &Entity<InputState>, cx: &App) -> String {
        e.read(cx).value().to_string()
    }

    /// 记录字段级错误并返回同一文案，供 build 的 `?` 传播到 Toast。
    fn fail(&self, key: &'static str, msg: impl Into<String>) -> String {
        let msg = msg.into();
        self.field_errors.borrow_mut().insert(key, msg.clone());
        msg
    }

    /// 把 Result 的错误挂到指定字段上再继续 `?` 传播。
    fn fail_as<T>(
        &self,
        key: &'static str,
        r: Result<T, String>,
    ) -> Result<T, String> {
        r.map_err(|e| self.fail(key, e))
    }

    fn parse_u16(s: &str, label: &'static str) -> Result<u16, String> {
        s.trim()
            .parse::<u16>()
            .map_err(|_| format!("{} {}", i18n::t(label), i18n::t("form.err_range")))
    }

    /// 端口：1~65535。0 不是可连接端口，放行只会在 CONNECT 阶段报出难以理解的错误。
    fn parse_port(s: &str) -> Result<u16, String> {
        match s.trim().parse::<u16>() {
            Ok(0) => Err(i18n::t("form.port_invalid").to_string()),
            Ok(p) => Ok(p),
            Err(_) => Err(i18n::t("form.port_invalid").to_string()),
        }
    }

    /// 同 `parse_opt_u32`，但 0 视为非法：MQTT 5 规范把 Receive Maximum /
    /// Maximum Packet Size 为 0 定义为 Protocol Error，broker 会直接断开连接，
    /// 且该值会被持久化，表现为"每次连接都失败"。
    fn parse_opt_positive_u32(s: &str, label: &'static str) -> Result<Option<u32>, String> {
        match Self::parse_opt_u32(s, label)? {
            Some(0) => Err(format!("{} {}", i18n::t(label), i18n::t("form.err_positive"))),
            other => Ok(other),
        }
    }

    fn parse_opt_u32(s: &str, label: &'static str) -> Result<Option<u32>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        s.parse::<u32>()
            .map(Some)
            .map_err(|_| format!("{} {}", i18n::t(label), i18n::t("form.err_uint")))
    }

    pub fn build(&self, cx: &App) -> Result<ConnectionConfig, String> {
        self.build_inner(cx, false)
    }

    /// 测试连接专用的宽松构建：名称与 Client ID 不阻塞测试
    ///（握手时会生成临时唯一 Client ID）。
    fn build_for_test(&self, cx: &App) -> Result<ConnectionConfig, String> {
        self.build_inner(cx, true)
    }

    fn build_inner(&self, cx: &App, for_test: bool) -> Result<ConnectionConfig, String> {
        // 每次构建先清空上一轮字段错误，失败时由 fail() 重新登记
        self.field_errors.borrow_mut().clear();

        let mut name = self.val(&self.name, cx);
        if name.trim().is_empty() {
            if !for_test {
                return Err(self.fail("name", i18n::t("form.name_required")));
            }
            name = i18n::t("form.test").into();
        }
        // 判空已按 trim，保存也存 trim 后的值，避免首尾空白入库
        let name = name.trim().to_string();
        let host = self.val(&self.host, cx);
        if host.trim().is_empty() {
            return Err(self.fail("host", i18n::t("form.host_required")));
        }
        let host = host.trim().to_string();
        let port = self.fail_as("port", Self::parse_port(&self.val(&self.port, cx)))?;
        let mut client_id = self.val(&self.client_id, cx);
        if client_id.trim().is_empty() {
            if !for_test {
                return Err(self.fail("client_id", i18n::t("form.client_id_required")));
            }
            client_id = format!("mqttx-test-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        }
        let client_id = client_id.trim().to_string();
        let keep_alive =
            self.fail_as("keep_alive", Self::parse_u16(&self.val(&self.keep_alive, cx), i18n::t("form.keep_alive_short")))?;
        // 空串视为 0（会话随连接结束）；非空必须是合法非负整数，不再静默吞掉 -1/abc
        let session_expiry = {
            let raw = self.val(&self.session_expiry, cx);
            let t = raw.trim();
            if t.is_empty() {
                0
            } else {
                t.parse::<u32>().map_err(|_| {
                    self.fail(
                        "session_expiry",
                        i18n::t("form.session_expiry_invalid"),
                    )
                })?
            }
        };
        let receive_maximum = self.fail_as("receive_max", (|| -> Result<_, String> {
            Ok(Self::parse_opt_positive_u32(&self.val(&self.receive_max, cx), i18n::t("form.receive_max_short"))?
                .map(|v| u16::try_from(v).map_err(|_| i18n::t("form.receive_max_invalid").to_string()))
                .transpose())
        })())??;
        let maximum_packet_size = self.fail_as(
            "max_packet",
            Self::parse_opt_positive_u32(&self.val(&self.max_packet, cx), i18n::t("form.max_packet")),
        )?;
        let topic_alias_maximum = self.fail_as("topic_alias", (|| -> Result<_, String> {
            Ok(Self::parse_opt_positive_u32(&self.val(&self.topic_alias, cx), i18n::t("form.topic_alias"))?
                .map(|v| u16::try_from(v).map_err(|_| i18n::t("form.topic_alias_invalid").to_string()))
                .transpose())
        })())??;

        let conn_timeout_raw = self.val(&self.conn_timeout, cx);
        let connection_timeout_secs = if conn_timeout_raw.trim().is_empty() {
            10
        } else {
            conn_timeout_raw
                .trim()
                .parse::<u16>()
                .ok()
                .filter(|v| *v >= 1)
                .ok_or_else(|| self.fail("conn_timeout", i18n::t("form.conn_timeout_invalid")))?
        };
        let max_reconnect_raw = self.val(&self.max_reconnect, cx);
        let max_reconnect_times = if max_reconnect_raw.trim().is_empty() {
            0
        } else {
            max_reconnect_raw
                .trim()
                .parse::<u32>()
                .map_err(|_| self.fail("max_reconnect", i18n::t("form.max_reconnects_invalid")))?
        };

        // 分组：新分组名非空时优先，否则取下拉所选（首项=无分组）
        let group = {
            let new_name = self.val(&self.group_new, cx);
            let trimmed = new_name.trim();
            if !trimmed.is_empty() {
                Some(trimmed.to_string())
            } else {
                let idx = self
                    .group_select
                    .read(cx)
                    .selected_value()
                    .copied()
                    .unwrap_or(0);
                if idx == 0 {
                    None
                } else {
                    self.groups.get(idx - 1).cloned()
                }
            }
        };

        let protocol = match self.protocol.read(cx).selected_value() {
            Some(1) => ProtocolVersion::V311,
            _ => ProtocolVersion::V5,
        };
        let transport = self
            .transport
            .read(cx)
            .selected_value()
            .and_then(|i| TransportKind::ALL.get(*i).copied())
            .unwrap_or_default();

        // WS/WSS 依赖 path 建立升级请求，缺失时连接必然失败，保存前拦截
        let path = self.val(&self.path, cx).trim().to_string();
        if transport.is_websocket() && path.is_empty() {
            return Err(self.fail("path", i18n::t("form.ws_path_required")));
        }

        let ssl = SslConfig {
            ca_file: self.val(&self.ssl_ca, cx).trim().to_string(),
            client_cert_file: self.val(&self.ssl_client_cert, cx).trim().to_string(),
            client_key_file: self.val(&self.ssl_client_key, cx).trim().to_string(),
            ignore_ca: self.ssl_ignore_ca,
        };
        // 只填证书或只填私钥必然握手失败；仅 TLS/WSS 下 SSL 生效，按生效传输校验
        if transport.is_tls()
            && ssl.client_cert_file.is_empty() != ssl.client_key_file.is_empty()
        {
            return Err(self.fail("ssl_client_cert", i18n::t("form.cert_pair_required")));
        }
        // 证书文件不存在时握手才报错太晚，保存前给出字段级提示
        if transport.is_tls() {
            for (key, label, p) in [
                ("ssl_ca", "CA 证书", &ssl.ca_file),
                ("ssl_client_cert", "客户端证书", &ssl.client_cert_file),
                ("ssl_client_key", "客户端私钥", &ssl.client_key_file),
            ] {
                if !p.is_empty() && !std::path::Path::new(p).exists() {
                    return Err(self.fail(key, i18n::tf("form.path_missing", &[("label", &i18n::t(label)), ("p", &p)])));
                }
            }
        }

        let last_will = if self.will_enabled {
            // 与其它字段一致：trim 后入库
            let topic = self.val(&self.will_topic, cx).trim().to_string();
            if topic.is_empty() {
                return Err(self.fail("will_topic", i18n::t("will.topic_required")));
            }
            // 通配符只用于订阅过滤器；出现在 PUBLISH 主题上会被 broker 直接拒绝
            if topic.contains('+') || topic.contains('#') {
                return Err(self.fail(
                    "will_topic",
                    i18n::t("will.topic_wildcard"),
                ));
            }
            let opt_str = |v: String| {
                let t = v.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            };
            Some(LastWill {
                topic,
                payload: self.val(&self.will_payload, cx),
                qos: self.will_qos.read(cx).selected_value().copied().unwrap_or(0) as u8,
                retain: self.will_retain,
                content_type: opt_str(self.val(&self.will_content_type, cx)),
                response_topic: opt_str(self.val(&self.will_response_topic, cx)),
            })
        } else {
            None
        };

        let id = self.editing_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Ok(ConnectionConfig {
            id,
            name,
            host,
            port,
            path,
            protocol,
            transport,
            client_id,
            username: self.val(&self.username, cx),
            password: self.val(&self.password, cx),
            clean_start: self.clean_start,
            keep_alive,
            session_expiry_interval: session_expiry,
            receive_maximum,
            maximum_packet_size,
            topic_alias_maximum,
            auto_resubscribe: self.auto_resubscribe,
            auto_reconnect: self.auto_reconnect,
            auto_connect: self.auto_connect,
            last_will,
            created_at: self.created_at,
            group,
            connection_timeout_secs,
            max_reconnect_times,
            ssl,
        })
    }

    pub fn run_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut cfg = match self.build_for_test(cx) {
            Ok(c) => c,
            Err(e) => {
                window.push_notification(Notification::error(e), cx);
                // 触发重绘，让刚登记的字段级红字立即显示
                cx.notify();
                return;
            }
        };
        // 测试连接用临时 id，不影响已有连接
        cfg.id = uuid::Uuid::new_v4().to_string();
        let rx = self.engine.test_connection(cfg);
        self.testing = true;
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let result = rx.recv().await.unwrap_or(Err("测试任务丢失".into()));
            this.update_in(cx, |form, window, cx| {
                form.testing = false;
                match result {
                    Ok(()) => window.push_notification(
                        Notification::success("连接测试成功"),
                        cx,
                    ),
                    Err(e) => window.push_notification(Notification::error(format!(
                        "连接测试失败: {e}"
                    )), cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 证书路径字段 + 「浏览…」按钮：路径仍可手输，按钮负责选文件。
    fn cert_path_row(
        &self,
        id: &'static str,
        label: &'static str,
        target: Entity<InputState>,
        err: Option<SharedString>,
        danger: gpui_kit::Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .gap_1()
            .w_full()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(field_ex(label, Input::new(&target), false, err, danger)),
            )
            .child(
                Button::new(id)
                    .label(i18n::t("common.browse"))
                    .outline()
                    .small()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.browse_cert_path(target.clone(), window, cx);
                    })),
            )
    }

    /// 弹原生文件对话框并把所选路径回填到输入框。rfd 的阻塞式对话框
    /// 不可占用 UI 线程，与连接导入/导出同模式：后台线程 + smol 通道回传。
    fn browse_cert_path(
        &self,
        target: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (tx, rx) = smol::channel::bounded::<Option<std::path::PathBuf>>(1);
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_title(i18n::t("ssl.pick_file"))
                .add_filter("证书 / 密钥 (PEM)", &["pem", "crt", "cer", "key", "der"])
                .pick_file();
            let _ = tx.send_blocking(picked);
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Some(path)) = rx.recv().await {
                let text = path.display().to_string();
                this.update_in(cx, |_form, window, cx| {
                    target.update(cx, |s, cx| s.set_value(text, window, cx));
                    // 回填的路径可能本就带字段错误提示，触发重绘即时更新
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn switch_row(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .justify_between()
            .child(div().text_sm().child(label))
            .child(
                Switch::new(id)
                    .checked(checked)
                    .on_change(cx.listener(move |this, v, _, cx| {
                        let v = *v;
                        match id {
                            "clean_start" => this.clean_start = v,
                            "auto_resubscribe" => this.auto_resubscribe = v,
                            "auto_connect" => this.auto_connect = v,
                            "auto_reconnect" => this.auto_reconnect = v,
                            "will_enabled" => this.will_enabled = v,
                            "will_retain" => this.will_retain = v,
                            "ssl_ignore_ca" => this.ssl_ignore_ca = v,
                            _ => {}
                        }
                        cx.notify();
                    })),
            )
    }

    /// 可折叠分组：点击标题行切换展开/收起，收起时不渲染内容。
    #[allow(clippy::too_many_arguments)]
    fn collapsible_section(
        &self,
        id: &'static str,
        title: &'static str,
        hint: Option<SharedString>,
        hint_accent: bool,
        open: bool,
        toggle: fn(&mut Self),
        border: gpui_kit::Hsla,
        bg: gpui_kit::Hsla,
        body: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let title: SharedString = title.into();
        v_flex().gap_2().child(
            v_flex()
                .child(
                    h_flex()
                        .id(id)
                        .justify_between()
                        .rounded_md()
                        .px_1()
                        .py_1()
                        .cursor_pointer()
                        .hover(|s| s.bg(cx.theme().secondary_hover))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            toggle(this);
                            cx.notify();
                        }))
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .child(div().text_sm().font_semibold().child(title))
                                .children(hint.map(|h| {
                                    let color = if hint_accent {
                                        cx.theme().accent
                                    } else {
                                        cx.theme().muted_foreground
                                    };
                                    div().text_xs().text_color(color).child(h)
                                })),
                        )
                        .child(
                            Icon::new(IconName::ChevronDown)
                                .xsmall()
                                .text_color(cx.theme().muted_foreground)
                                .rotate(percentage(if open { 0.5 } else { 0. })),
                        ),
                )
                .when(open, |w| {
                    w.child(
                        div()
                            .rounded_lg()
                            .border_1()
                            .border_color(border)
                            .bg(bg)
                            .p_3()
                            .child(body),
                    )
                }),
        )
    }
}

impl Render for ConnectionForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let card_bg = cx.theme().secondary;
        // 协议/传输决定高级分组的按需显示
        let is_v5 = self.protocol.read(cx).selected_value() != Some(&1);
        let is_ws = matches!(
            self.transport.read(cx).selected_value(),
            Some(2) | Some(3)
        );
        // SSL/TLS 区块只对 TLS/WSS 有意义
        let is_tls = matches!(
            self.transport.read(cx).selected_value(),
            Some(1) | Some(3)
        );
        // build() 校验失败登记的字段级错误，渲染成红字；红/星号用主题错误色
        let danger = cx.theme().danger;
        let field_errors = self.field_errors.borrow().clone();
        let err = move |key: &'static str| -> Option<SharedString> {
            field_errors.get(key).map(|s| SharedString::from(s.as_str()))
        };
        // 折叠标题的「已配置 N 项」提示，避免编辑旧配置时看不到已填内容
        let ssl_count = [
            self.val(&self.ssl_ca, cx).trim().is_empty(),
            self.val(&self.ssl_client_cert, cx).trim().is_empty(),
            self.val(&self.ssl_client_key, cx).trim().is_empty(),
        ]
        .iter()
        .filter(|empty| !**empty)
        .count();
        let mqtt5_count = [
            !self.val(&self.session_expiry, cx).trim().is_empty(),
            !self.val(&self.receive_max, cx).trim().is_empty(),
            !self.val(&self.max_packet, cx).trim().is_empty(),
            !self.val(&self.topic_alias, cx).trim().is_empty(),
        ]
        .iter()
        .filter(|on| **on)
        .count();
        let ssl_hint = (ssl_count > 0).then(|| SharedString::from(i18n::tf("ssl.n_configured", &[("n", &ssl_count)])));
        let mqtt5_hint =
            (mqtt5_count > 0).then(|| SharedString::from(i18n::tf("ssl.n_configured", &[("n", &mqtt5_count)])));
        v_flex()
            .gap_3()
            .max_h(px(560.))
            .overflow_y_scrollbar()
            .pr_1()
            // ── 基本信息（始终展开） ──
            .child(section(i18n::t("form.basics"), border, card_bg,
                v_flex().gap_2().child(
                    // 主机与端口是一对，放同一行；名称与协议/传输同行
                    h_flex().gap_2().w_full()
                        .child(div().flex_1().min_w(px(0.)).child(field_ex(i18n::t("form.host"), Input::new(&self.host), true, err("host"), danger)))
                        .child(div().w(px(120.)).child(field_ex(i18n::t(i18n::t("form.port")), Input::new(&self.port), true, err("port"), danger)))
                )
                .child(h_flex().gap_2().w_full()
                    .child(div().flex_1().min_w(px(0.)).child(field_ex(i18n::t("form.name"), Input::new(&self.name), true, err("name"), danger)))
                    .child(div().flex_1().min_w(px(0.)).child(field(i18n::t("form.protocol"), Select::new(&self.protocol))))
                    .child(div().flex_1().min_w(px(0.)).child(field(i18n::t("form.transport"), Select::new(&self.transport))))
                )
                .child(h_flex().gap_2().w_full()
                    .child(div().flex_1().min_w(px(0.)).child(field(i18n::t("form.group"), Select::new(&self.group_select))))
                    .child(div().flex_1().min_w(px(0.)).child(field(i18n::t("form.new_group"), Input::new(&self.group_new))))
                )
                .child(h_flex().gap_2().w_full().items_end()
                    .child(div().flex_1().min_w(px(0.)).child(field_ex(i18n::t("form.client_id"), Input::new(&self.client_id), true, err("client_id"), danger)))
                    // 一键换一个随机 Client ID，与 ConnectionConfig::new 的生成逻辑一致
                    .child(
                        Button::new("cid-regen")
                            .icon(IconName::RefreshCw)
                            .ghost()
                            .tooltip(i18n::t("form.regen_client_id"))
                            .accessibility_label(i18n::t("form.regen_client_id"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let cid = crate::model::generate_client_id();
                                this.client_id
                                    .update(cx, |s, cx| s.set_value(cid, window, cx));
                                cx.notify();
                            })),
                    )
                    // WS 路径仅对 WebSocket/WSS 有意义，按需显示；该传输下必填
                    .when(is_ws, |w| w.child(
                        div().flex_1().min_w(px(0.)).child(field_ex(i18n::t("form.ws_path"), Input::new(&self.path), true, err("path"), danger))
                    ))
                )
            ))
            // ── 认证与心跳 ──
            .child(self.collapsible_section(
                "sec-auth", i18n::t("form.auth_section"), None, false, self.show_auth,
                |f| f.show_auth = !f.show_auth, border, card_bg,
                v_flex().gap_2()
                    .child(h_flex().gap_2().w_full()
                        .child(div().flex_1().min_w(px(0.)).child(field(i18n::t("form.username"), Input::new(&self.username))))
                        .child(div().flex_1().min_w(px(0.)).child(field(i18n::t("form.password"), Input::new(&self.password).mask_toggle())))
                        .child(div().w(px(110.)).child(field_ex(i18n::t("form.keep_alive"), Input::new(&self.keep_alive), true, err("keep_alive"), danger)))
                    )
                    .child(h_flex().gap_2().w_full()
                        .child(div().w(px(150.)).child(field_ex(i18n::t("form.conn_timeout"), Input::new(&self.conn_timeout), false, err("conn_timeout"), danger)))
                        .child(div().w(px(170.)).child(field_ex(i18n::t("form.max_reconnects"), Input::new(&self.max_reconnect), false, err("max_reconnect"), danger)))
                    )
                    .child(self.switch_row("clean_start", i18n::t("form.clean_start"), self.clean_start, cx))
                    .child(self.switch_row("auto_reconnect", i18n::t("form.auto_reconnect"), self.auto_reconnect, cx))
                    .child(self.switch_row("auto_resubscribe", i18n::t("form.auto_resub"), self.auto_resubscribe, cx))
                    .child(self.switch_row("auto_connect", i18n::t("form.auto_connect"), self.auto_connect, cx)),
                cx,
            ))
            // ── SSL/TLS（仅 TLS/WSS 显示） ──
            .when(is_tls, |w| w.child(self.collapsible_section(
                "sec-ssl", i18n::t("ssl.section"), ssl_hint, true, self.show_ssl,
                |f| f.show_ssl = !f.show_ssl, border, card_bg,
                v_flex().gap_2()
                    .child(self.cert_path_row(
                        "ssl-ca-browse", i18n::t("ssl.ca"), self.ssl_ca.clone(),
                        err("ssl_ca"), danger, cx,
                    ))
                    .child(self.cert_path_row(
                        "ssl-cert-browse", i18n::t("ssl.cert"), self.ssl_client_cert.clone(),
                        err("ssl_client_cert"), danger, cx,
                    ))
                    .child(self.cert_path_row(
                        "ssl-key-browse", i18n::t("ssl.key"), self.ssl_client_key.clone(),
                        err("ssl_client_key"), danger, cx,
                    ))
                    .child(self.switch_row("ssl_ignore_ca", i18n::t("ssl.skip_verify"), self.ssl_ignore_ca, cx)),
                cx,
            )))
            // ── MQTT 5 高级属性（仅 v5 协议显示） ──
            .when(is_v5, |w| w.child(self.collapsible_section(
                "sec-mqtt5", "MQTT 5 属性", mqtt5_hint, true, self.show_mqtt5,
                |f| f.show_mqtt5 = !f.show_mqtt5, border, card_bg,
                h_flex().gap_2().w_full()
                    .child(div().flex_1().min_w(px(0.)).child(field_ex(i18n::t("form.session_expiry"), Input::new(&self.session_expiry), false, err("session_expiry"), danger)))
                    .child(div().flex_1().min_w(px(0.)).child(field_ex(i18n::t("form.receive_max"), Input::new(&self.receive_max), false, err("receive_max"), danger)))
                    .child(div().flex_1().min_w(px(0.)).child(field_ex(i18n::t("form.max_packet"), Input::new(&self.max_packet), false, err("max_packet"), danger)))
                    .child(div().flex_1().min_w(px(0.)).child(field_ex(i18n::t("form.topic_alias_max"), Input::new(&self.topic_alias), false, err("topic_alias"), danger))),
                cx,
            )))
            // ── 遗嘱 ──
            .child(self.collapsible_section(
                "sec-will", i18n::t("form.will_section"),
                self.will_enabled.then(|| i18n::t("will.enabled_hint").into()), true, self.show_will,
                |f| f.show_will = !f.show_will, border, card_bg,
                v_flex().gap_2()
                    .child(self.switch_row("will_enabled", i18n::t("form.will_enable"), self.will_enabled, cx))
                    .when(self.will_enabled, |w| w
                        .child(h_flex().gap_2()
                            .child(div().flex_1().min_w(px(0.)).child(field_ex(i18n::t("form.will_topic"), Input::new(&self.will_topic), true, err("will_topic"), danger)))
                            .child(div().w(px(130.)).child(field("QoS", Select::new(&self.will_qos))))
                        )
                        .child(field(i18n::t("form.will_payload"), Input::new(&self.will_payload)))
                        // v5 遗嘱属性；3.1.1 连接会忽略这两项
                        .child(h_flex().gap_2().w_full()
                            .child(div().flex_1().min_w(px(0.)).child(field(i18n::t("form.will_content_type"), Input::new(&self.will_content_type))))
                            .child(div().flex_1().min_w(px(0.)).child(field(i18n::t("form.will_response_topic"), Input::new(&self.will_response_topic))))
                        )
                        .child(self.switch_row("will_retain", "Retain", self.will_retain, cx))
                    ),
                cx,
            ))
    }
}

fn section(
    title: impl Into<gpui_kit::SharedString>,
    border: gpui_kit::Hsla,
    bg: gpui_kit::Hsla,
    body: impl IntoElement,
) -> impl IntoElement {
    let title: gpui_kit::SharedString = title.into();
    v_flex()
        .gap_2()
        .child(div().text_sm().font_semibold().child(title))
        .child(
            div()
                .rounded_lg()
                .border_1()
                .border_color(border)
                .bg(bg)
                .p_3()
                .child(body),
        )
}
