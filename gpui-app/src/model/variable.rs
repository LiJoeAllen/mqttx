//! 全局变量：模板占位符渲染（含 {{timestamp}} 时间基一致性）。


use serde::{Deserialize, Serialize};


use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalVariable {
    pub key: String,
    pub value: String,
}

/// 解析 `{{key}}` 模板：
/// - 用户定义的全局变量优先；
/// - 内置变量：`{{$ts}}`（unix 秒）、`{{$ts_ms}}`（毫秒）、`{{$uuid}}`；
/// - 找不到的占位符保持原样。
pub fn render_template(input: &str, vars: &[GlobalVariable]) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    // 同一次渲染内时间基准保持一致，避免一条消息里多个时间变量取值不同。
    let now_ms = chrono::Local::now().timestamp_millis();
    let now_s = now_ms / 1000;
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{'
            && let Some(rel_end) = input[i + 2..].find("}}") {
                let key = &input[i + 2..i + 2 + rel_end];
                let resolved = match key.trim() {
                    "$ts" => Some(now_s.to_string()),
                    "$ts_ms" => Some(now_ms.to_string()),
                    "$uuid" => Some(uuid::Uuid::new_v4().to_string()),
                    k => vars
                        .iter()
                        .find(|v| v.key == k)
                        .map(|v| v.value.clone()),
                };
                match resolved {
                    Some(v) => out.push_str(&v),
                    None => out.push_str(&input[i..i + 2 + rel_end + 2]),
                }
                i = i + 2 + rel_end + 2;
                continue;
            }
        // 按字符推进，避免把多字节 UTF-8 拆开
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// 连接前渲染遗嘱消息中的 `{{变量}}`。
/// 只作用于本次连接的配置副本，落盘的连接配置保持模板原文，
/// 这样改全局变量后下次重连即生效，`$ts` 等也按连接时刻求值。
pub fn render_will_templates(cfg: &mut ConnectionConfig, vars: &[GlobalVariable]) {
    let Some(will) = cfg.last_will.as_mut() else {
        return;
    };
    will.topic = render_template(&will.topic, vars);
    will.payload = render_template(&will.payload, vars);
    if let Some(v) = will.content_type.take() {
        will.content_type = Some(render_template(&v, vars));
    }
    if let Some(v) = will.response_topic.take() {
        will.response_topic = Some(render_template(&v, vars));
    }
}

// ─── 日志 ────────────────────────────────────────────────────────────────────
