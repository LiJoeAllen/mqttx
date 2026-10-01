//! 纯函数工具：topic 匹配、占位符提取、载荷预览/复制文本、订阅配色。


use gpui_kit::{ hsla, Hsla,  };

use crate::model::{ MqttRecord, Subscription,  };
use crate::ui::widgets::format_time;

use super::*;

/// 色相（度）→ gpui Hsla（h 分量为 0..=1 的整圆比例）。
/// 固定 L=0.52 时亮色相（黄绿）在浅色主题下过亮、暗色相（蓝靛）在深色主题下过暗，
/// 按色相段微调明度，保证两端主题下都可辨识。
pub(super) fn hue_color(hue: f32) -> Hsla {
    let h = hue.rem_euclid(360.0);
    let l = if (30.0..=90.0).contains(&h) {
        // 黄/黄绿本身明度感高，压低一点防刺眼
        0.44
    } else if (200.0..=280.0).contains(&h) {
        // 蓝/靛本身明度感低，抬高一点防发黑
        0.60
    } else {
        0.52
    };
    hsla(h / 360.0, 0.72, l, 1.0)
}

/// 逗号 / 空白 / 换行分隔的多主题输入 → 去重后的主题列表（批量订阅）
pub(super) fn split_topic_list(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in raw.split(|c: char| c == ',' || c.is_whitespace()) {
        let t = part.trim();
        if !t.is_empty() && !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// 从发布主题与负载中提取 `{{key}}` 占位符的 key 列表：
/// - key 两侧空白去除（trim）；
/// - `$` 开头的内置变量（`$ts` / `$ts_ms` / `$uuid`）不提取；
/// - 重复 key 去重，保持首次出现顺序（主题在前、负载在后）。
pub(super) fn extract_placeholder_keys(topic: &str, payload: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for text in [topic, payload] {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b'{' && bytes[i + 1] == b'{' && let Some(rel_end) = text[i + 2..].find("}}") {
                let key = text[i + 2..i + 2 + rel_end].trim();
                if !key.is_empty() && !key.starts_with('$') && !out.iter().any(|k| k == key) {
                    out.push(key.to_string());
                }
                i = i + 2 + rel_end + 2;
                continue;
            }
            // 按字符推进，避免把多字节 UTF-8 拆开
            let ch = text[i..].chars().next().unwrap();
            i += ch.len_utf8();
        }
    }
    out
}

/// MQTT 主题过滤器匹配：`+` 匹配单层，`#` 匹配本层及以下（可匹配父级本身）。
pub(super) fn topic_matches(filter: &str, topic: &str) -> bool {
    let f: Vec<&str> = filter.split('/').collect();
    let t: Vec<&str> = topic.split('/').collect();
    for (i, part) in f.iter().enumerate() {
        if *part == "#" {
            // "a/#" 需要匹配 "a" 自身：剩余层级数 >= 过滤器已消费层数
            return i <= t.len();
        }
        if i >= t.len() {
            return false;
        }
        if *part != "+" && *part != t[i] {
            return false;
        }
    }
    f.len() == t.len()
}

/// 挑选消息着色用的订阅色：先看精确匹配，再看通配匹配；只考虑启用且已设色的订阅。
pub(super) fn pick_subscription_color(subs: &[Subscription], topic: &str) -> Option<f32> {
    let has_wild = |t: &str| t.contains('+') || t.contains('#');
    for s in subs.iter().filter(|s| s.enabled) {
        if s.topic == topic && !has_wild(&s.topic) && s.color.is_some() {
            return s.color;
        }
    }
    for s in subs.iter().filter(|s| s.enabled) {
        if has_wild(&s.topic) && topic_matches(&s.topic, topic) && s.color.is_some() {
            return s.color;
        }
    }
    None
}

/// 展开详情里的 payload 渲染：自动 = 合法 JSON 美化，否则原文。
/// Hex/Base64 优先使用入库保留的原始字节——非文本负载的 lossy 文本
/// 已把非法字节替换为 U+FFFD，按文本字节展示会与线上报文不符。
pub(super) fn format_payload_detail(payload: &str, raw: Option<&[u8]>, format: DetailFormat) -> String {
    match format {
        DetailFormat::Auto => serde_json::from_str::<serde_json::Value>(payload)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| payload.to_string()),
        DetailFormat::Text => payload.to_string(),
        DetailFormat::Hex => raw
            .unwrap_or(payload.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" "),
        DetailFormat::Base64 => {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.encode(raw.unwrap_or(payload.as_bytes()))
        }
    }
}

/// 「复制详情」的可读文本：元数据 + v5 属性 + 用户属性 + 负载。
pub(super) fn details_copy_text(record: &MqttRecord, show_millis: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!("Topic: {}\n", record.topic));
    out.push_str(&format!("Direction: {}\n", record.direction.label()));
    out.push_str(&format!("QoS: {}\n", record.qos));
    out.push_str(&format!("Retain: {}\n", record.retain));
    out.push_str(&format!(
        "Time: {}\n",
        format_time(record.timestamp, show_millis)
    ));
    if let Some(v) = record.content_type.as_deref() {
        out.push_str(&format!("Content-Type: {v}\n"));
    }
    if let Some(v) = record.response_topic.as_deref() {
        out.push_str(&format!("Response Topic: {v}\n"));
    }
    if let Some(v) = record.correlation_data.as_deref() {
        out.push_str(&format!("Correlation Data: {v}\n"));
    }
    if let Some(v) = record.message_expiry_interval {
        out.push_str(&format!("Message Expiry Interval: {v}\n"));
    }
    if let Some(v) = record.subscription_identifier {
        out.push_str(&format!("Subscription Identifier: {v}\n"));
    }
    if !record.user_properties.is_empty() {
        out.push_str("User Properties:\n");
        for (k, v) in record.user_properties.iter() {
            out.push_str(&format!("  {k} = {v}\n"));
        }
    }
    out.push_str(&format!("Payload:\n{}", record.payload));
    out
}

/// 大小写不敏感子串匹配。`needle` 已由调用方 to_lowercase 一次；
/// 先做零拷贝的字面匹配，未命中且原文含大写时才回退整串小写，
/// 避免渲染热路径对全部记录做 to_lowercase 拷贝。
pub(super) fn contains_ignore_case(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if hay.contains(needle) {
        return true;
    }
    // 纯 ASCII 快速路径：双方均为 ASCII 且原文无 ASCII 大写时，
    // 小写化恒等，可直接判否。不能用 is_uppercase 判断——个别
    // 小写字符（如 U+1E9B）的小写映射并不等于自身。
    if needle.is_ascii()
        && hay.is_ascii()
        && !hay.bytes().any(|b| b.is_ascii_uppercase())
    {
        return false;
    }
    hay.to_lowercase().contains(needle)
}

// ─── 单元测试 ────────────────────────────────────────────────────────────────


#[cfg(test)]
mod tests {
    use super::extract_placeholder_keys;

    // 修复回归：提取须 trim、排除内置变量、去重且保持首次出现顺序
    #[test]
    fn extract_keys_trims_excludes_builtin_and_dedups() {
        let keys = extract_placeholder_keys(
            "sensor/{{ device }}/state",
            "{\"v\":{{value}},\"t\":{{ $ts }},\"u\":{{uuid}}}",
        );
        assert_eq!(keys, vec!["device", "value", "uuid"], "实际: {keys:?}");
    }

    #[test]
    fn extract_keys_empty_and_multibyte_safe() {
        assert!(extract_placeholder_keys("", "").is_empty());
        assert!(extract_placeholder_keys("no placeholders here", "纯文本").is_empty());
        // 未闭合的 {{ 不提取、不越界
        assert!(extract_placeholder_keys("a{{b", "c}}d").is_empty());
        let keys = extract_placeholder_keys("温度{{名称}}", "");
        assert_eq!(keys, vec!["名称"]);
    }

}
