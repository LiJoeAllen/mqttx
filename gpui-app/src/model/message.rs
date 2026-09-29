//! 消息：方向、载荷格式、消息记录与条数/字节双预算的环形缓冲。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use std::collections::VecDeque;


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Received,
    Published,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Received => "接收",
            Self::Published => "发布",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PayloadFormat {
    #[default]
    Plaintext,
    Json,
    Base64,
    Hex,
}

impl PayloadFormat {
    pub const ALL: [Self; 4] = [Self::Plaintext, Self::Json, Self::Base64, Self::Hex];

    pub fn label(self) -> &'static str {
        match self {
            Self::Plaintext => "Plaintext",
            Self::Json => "JSON",
            Self::Base64 => "Base64",
            Self::Hex => "Hex",
        }
    }

    pub fn encode(self, text: &str) -> Result<Vec<u8>, String> {
        match self {
            Self::Plaintext | Self::Json => Ok(text.as_bytes().to_vec()),
            Self::Base64 => {
                use base64::Engine as _;
                base64::engine::general_purpose::STANDARD
                    .decode(text.trim())
                    .map_err(|e| format!("Base64 解码失败: {e}"))
            }
            Self::Hex => {
                let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                if !cleaned.len().is_multiple_of(2) {
                    return Err("Hex 长度必须为偶数（每两个字符一个字节）".into());
                }
                // 必须先做全字节校验：长度检查按字节计数，多字节字符（如中文）
                // 可凑成偶数字节，直接按字节索引切片会落在字符边界内而 panic。
                if !cleaned.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("Hex 解码失败: 含非十六进制字符".into());
                }
                (0..cleaned.len())
                    .step_by(2)
                    .map(|i| {
                        u8::from_str_radix(&cleaned[i..i + 2], 16)
                            .map_err(|e| format!("Hex 解码失败: {e}"))
                    })
                    .collect()
            }
        }
    }
}


/// 一条收到/发出的消息记录。payload 以 UTF-8 文本保存（非文本按 lossy 显示）。
///
/// 内存优化：`connection_id`/`topic` 用 `Arc<str>`——同一连接内主题高度重复，
/// 驻留后数千条记录只保留一份字符串。
#[derive(Debug, Clone)]
pub struct MqttRecord {
    pub seq: u64,
    pub connection_id: Arc<str>,
    pub direction: Direction,
    pub topic: Arc<str>,
    /// Arc 共享：消息列表渲染每帧克隆最多数百条记录，零拷贝
    pub payload: Arc<str>,
    pub qos: u8,
    pub retain: bool,
    /// unix 毫秒
    pub timestamp: i64,
    pub user_properties: Vec<(String, String)>,
    pub content_type: Option<String>,
    pub response_topic: Option<String>,
    pub correlation_data: Option<String>,
    pub message_expiry_interval: Option<u32>,
    pub subscription_identifier: Option<u32>,
    /// payload 超过 [`MAX_PAYLOAD_RETAIN`] 被截断
    pub payload_truncated: bool,
    /// 非文本负载的原始字节（合法 UTF-8 时为 None）。文本化会失真
    /// （非法字节被替换为 U+FFFD），详情面板的 Hex/Base64 需要真实报文。
    pub raw_bytes: Option<Arc<[u8]>>,
}

impl MqttRecord {
    /// 该记录保留的主要字节数（文本 + 原始字节），用于内存预算驱逐。
    /// 其余字段（主题/属性等）相对小一个数量级，不计入。
    pub fn retained_bytes(&self) -> usize {
        self.payload.len() + self.raw_bytes.as_ref().map_or(0, |b| b.len())
    }
}

/// 单连接的消息环形缓冲：条数上限 + [`MAX_CONNECTION_MESSAGE_BYTES`]
/// 字节上限双重驱逐（从最旧开始，均摊 O(1)）。
pub struct MessageRing {
    queue: VecDeque<MqttRecord>,
    bytes: usize,
}

impl MessageRing {
    pub fn new(max_count: usize) -> Self {
        Self {
            // 预分配按较小值钳制：设置上限十万条时不一次性分配过大
            queue: VecDeque::with_capacity(max_count.clamp(16, 512)),
            bytes: 0,
        }
    }

    /// `max_count` 每次传入以便运行时调整设置后即时生效。
    /// 设置层已校验 ≥100，这里只防 0 值导致缓冲被清空。
    pub fn push(&mut self, record: MqttRecord, max_count: usize) {
        let max_count = max_count.max(1);
        self.bytes += record.retained_bytes();
        while self.queue.len() >= max_count
            || (self.bytes > MAX_CONNECTION_MESSAGE_BYTES && self.queue.len() > 1)
        {
            let Some(old) = self.queue.pop_front() else { break };
            self.bytes -= old.retained_bytes();
        }
        self.queue.push_back(record);
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// 当前保留的字节数（payload + 原始字节）。
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }

    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, MqttRecord> {
        self.queue.iter()
    }
}

/// 消息历史中单条 payload 的保留上限。超限截断，避免个别大报文
/// 长期占据历史缓冲（MQTTX 亦有类似限制）。
pub const MAX_PAYLOAD_RETAIN: usize = 128 * 1024;

/// 每连接消息历史的字节预算（payload + 原始字节）。条数上限
/// （max_messages）在大报文流下形同虚设（2000 条 × 128KB ≈ 256MB），
/// 超出预算后从最旧开始驱逐，保证单连接消息内存有上界。
pub const MAX_CONNECTION_MESSAGE_BYTES: usize = 32 * 1024 * 1024;

/// 把原始 payload 字节转成用于历史记录的共享文本，超限截断。
/// 返回 (文本, 非文本时的原始字节, 是否截断)。截断时回退到 UTF-8 字符边界。
///
/// 仅当文本化会失真（非合法 UTF-8，字节被替换为 U+FFFD）时才保留原始
/// 字节（同样受 [`MAX_PAYLOAD_RETAIN`] 限制）；纯文本负载零额外开销。
pub fn retained_payload(raw: &[u8]) -> (Arc<str>, Option<Arc<[u8]>>, bool) {
    let truncated = raw.len() > MAX_PAYLOAD_RETAIN;
    let text_part = if truncated {
        let mut end = MAX_PAYLOAD_RETAIN;
        while end > 0 && (raw[end] & 0xC0) == 0x80 {
            end -= 1;
        }
        &raw[..end]
    } else {
        raw
    };
    let raw_bytes = match std::str::from_utf8(text_part) {
        Ok(_) => None,
        Err(_) => Some(Arc::from(text_part.to_vec())),
    };
    let text: Arc<str> = Arc::from(String::from_utf8_lossy(text_part).as_ref());
    (text, raw_bytes, truncated)
}

// ─── 发布参数 / 发布预设 ──────────────────────────────────────────────────────
