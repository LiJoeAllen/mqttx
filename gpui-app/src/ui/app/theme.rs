//! 主题：加载 Linear 风格主题 JSON 并按偏好应用明暗模式。


use gpui_kit::component::{
    Theme, ThemeMode, ThemeRegistry,
};
use gpui_kit::App;

use crate::model::ThemeModePref;

/// 应用主题偏好。
///
/// 「跟随系统」读取 gpui 的窗口外观（`App::window_appearance`）；系统深浅色变化由
/// Linear 风格主题（内嵌 JSON，模式与 gpui-component 默认主题同构）。
const LINEAR_THEME: &str = include_str!("linear-theme.json");

/// `MqttXApp::new` 里注册的 `Window::observe_window_appearance` 监听并重应用本函数。
///
/// 流程：先 `change` 确保 Theme 全局已建立（注册表 observer 依赖它）→ 注册
/// Linear Light/Dark（重名跳过，幂等）→ 把当前明暗对指向 Linear → 再 `change`
/// 应用其颜色/字号/圆角。
pub fn apply_theme(pref: ThemeModePref, cx: &mut App) {
    let mode = match pref {
        ThemeModePref::Light => ThemeMode::Light,
        ThemeModePref::Dark => ThemeMode::Dark,
        ThemeModePref::System => ThemeMode::from(cx.window_appearance()),
    };
    Theme::change(mode, None, cx);
    if let Err(e) = ThemeRegistry::global_mut(cx).load_themes_from_str(LINEAR_THEME) {
        eprintln!("加载 Linear 主题失败，回退默认主题: {e}");
    }
    {
        let reg = ThemeRegistry::global(cx);
        let light = reg.themes().get("Linear Light").cloned();
        let dark = reg.themes().get("Linear Dark").cloned();
        let theme = Theme::global_mut(cx);
        if let Some(l) = light {
            theme.light_theme = l;
        }
        if let Some(d) = dark {
            theme.dark_theme = d;
        }
    }
    Theme::change(mode, None, cx);
}

// ─── 渲染 ────────────────────────────────────────────────────────────────────
