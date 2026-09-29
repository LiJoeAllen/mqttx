//! 基础部件：字段行布局、连接状态指示、时间格式化。

use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::{div, px, App, Hsla, IntoElement, ParentElement as _, SharedString, Styled as _};

use crate::model::ConnectionStatus;

/// 小号标签 + 控件的垂直字段容器（可选字段，无校验态）。
pub fn field(label: impl Into<SharedString>, control: impl IntoElement) -> impl IntoElement {
    field_ex(label, control, false, None, Hsla::default())
}

/// 字段行变体：`required` 时标签后追加红色 `*`；
/// `error` 为 Some 时在控件下方渲染红字提示（`danger` 为主题错误色）。
pub fn field_ex(
    label: impl Into<SharedString>,
    control: impl IntoElement,
    required: bool,
    error: Option<SharedString>,
    danger: Hsla,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .min_w(px(0.))
        .child(
            h_flex()
                .gap_0p5()
                .items_baseline()
                .child(div().text_xs().child(label.into()))
                .children(required.then(|| div().text_xs().text_color(danger).child("*"))),
        )
        .child(control)
        .children(error.map(|e| div().text_xs().text_color(danger).child(e)))
}

// ─── 状态颜色 ────────────────────────────────────────────────────────────────

pub fn status_color(status: ConnectionStatus, cx: &App) -> Hsla {
    let t = cx.theme();
    match status {
        ConnectionStatus::Connected => t.success,
        ConnectionStatus::Connecting => t.warning,
        ConnectionStatus::Error => t.danger,
        ConnectionStatus::Disconnected => t.muted_foreground,
    }
}

pub fn status_dot(status: ConnectionStatus, cx: &App) -> impl IntoElement {
    div()
        .size(px(8.))
        .rounded_full()
        .bg(status_color(status, cx))
}

// ─── 键值对编辑器 ────────────────────────────────────────────────────────────


pub fn format_time(ts_ms: i64, show_millis: bool) -> String {
    use chrono::TimeZone as _;
    let Some(dt) = chrono::Local.timestamp_millis_opt(ts_ms).single() else {
        return String::new();
    };
    if show_millis {
        dt.format("%H:%M:%S%.3f").to_string()
    } else {
        dt.format("%H:%M:%S").to_string()
    }
}
