//! 主题：Linear 自定义主题 + 官方捆绑预设（38 个）的注册与应用。
//!
//! 两套来源：
//! - **Linear**（`linear-theme.json`，浅/深一对）：默认预设，受「跟随系统 /
//!   浅色 / 深色」外观模式控制；
//! - **官方捆绑**（`themes/*.json`，36 个变体 + Default Light/Dark）：
//!   编译期内嵌、启动时注册进 ThemeRegistry，选择后整包应用（自带 mode）。
//!
//! 预设与外观模式是互斥维度：选定官方预设时由该预设决定全部配色与明暗，
//! 外观模式开关仅对 Linear 生效（系统深浅变化监听同样跳过官方预设，
//! 避免冲掉用户选择）。


use gpui_kit::component::{
    Theme, ThemeMode, ThemeRegistry,
};
use gpui_kit::App;

use crate::model::ThemeModePref;

/// 默认预设：Linear 风格主题（内嵌 JSON，模式与 gpui-component 默认主题同构）。
const LINEAR_THEME: &str = include_str!("linear-theme.json");

/// 官方捆绑预设（gpui-kit themes/，21 文件 36 变体 + Default Light/Dark）。
/// 编译期内嵌：单二进制发布不依赖运行时 themes 目录。
const BUNDLED_THEME_FILES: &[&str] = &[
    include_str!("../../../themes/adventure.json"),
    include_str!("../../../themes/alduin.json"),
    include_str!("../../../themes/asciinema.json"),
    include_str!("../../../themes/aurora.json"),
    include_str!("../../../themes/ayu.json"),
    include_str!("../../../themes/catppuccin.json"),
    include_str!("../../../themes/everforest.json"),
    include_str!("../../../themes/fahrenheit.json"),
    include_str!("../../../themes/flexoki.json"),
    include_str!("../../../themes/gruvbox.json"),
    include_str!("../../../themes/harper.json"),
    include_str!("../../../themes/hybrid.json"),
    include_str!("../../../themes/jellybeans.json"),
    include_str!("../../../themes/kibble.json"),
    include_str!("../../../themes/macos-classic.json"),
    include_str!("../../../themes/mellifluous.json"),
    include_str!("../../../themes/molokai.json"),
    include_str!("../../../themes/solarized.json"),
    include_str!("../../../themes/spaceduck.json"),
    include_str!("../../../themes/tokyonight.json"),
    include_str!("../../../themes/twilight.json"),
];

/// 默认预设名（settings.theme_preset 的初始值）。
pub const DEFAULT_PRESET: &str = "linear";
/// Linear 在预设列表中的展示名。
pub const LINEAR_PRESET_LABEL: &str = "Linear";

/// 应用主题偏好（Linear 预设专用路径）。
///
/// 「跟随系统」读取 gpui 的窗口外观；系统深浅色变化由
/// `MqttXApp::new` 里注册的 `Window::observe_window_appearance` 监听并重应用本函数。
///
/// 流程：先 `change` 确保 Theme 全局已建立（注册表 observer 依赖它）→ 注册
/// Linear Light/Dark（重名跳过，幂等）→ 把当前明暗对指向 Linear → 再 `change`
/// 应用其颜色/字号/圆角。
pub fn apply_theme(pref: ThemeModePref, cx: &mut App) {
    eprintln!("[THM] apply_theme enter");
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

/// 注册全部官方捆绑预设（幂等，重名由 ThemeRegistry 覆盖/跳过）。
/// 启动时调用一次。
pub fn register_bundled_themes(cx: &mut App) {
    eprintln!("[THM] register START");
    // [bisect] 只注册 catppuccin 4 个
    if let Err(e) = ThemeRegistry::global_mut(cx)
        .load_themes_from_str(include_str!("../../../themes/catppuccin.json"))
    {
        eprintln!("加载捆绑主题预设失败: {e}");
    }
    eprintln!("[THM] register catppuccin done");
    #[allow(clippy::needless_range_loop)]
    for _i in 0..0 {
    for json in BUNDLED_THEME_FILES {
        if let Err(e) = ThemeRegistry::global_mut(cx).load_themes_from_str(json) {
            eprintln!("加载捆绑主题预设失败: {e}");
        }
    }
    }
}

/// 应用官方捆绑预设（settings.theme_preset ≠ "linear" 时的路径）。
/// 整包应用：apply_config 连同预设自带的 mode 一起生效，
/// 经 Theme::update 走官方 reconcile + sync + 全窗刷新。
/// 名字不存在（理论不可达，测试拦截）时返回 false 回退 Linear。
pub fn apply_theme_preset(name: &str, cx: &mut App) -> bool {
    eprintln!("[THM] apply_theme_preset enter ({name})");
    let config = ThemeRegistry::global(cx).themes().get(name).cloned();
    let Some(config) = config else {
        return false;
    };
    Theme::update(cx, |theme| theme.apply_config(&config));
    true
}

/// 预设展示名单：Linear 在首，其余按名字排序（与 ThemeRegistry 一致）。
pub fn theme_preset_names(cx: &App) -> Vec<String> {
    eprintln!("[THM] theme_preset_names enter");
    let mut names: Vec<String> = ThemeRegistry::global(cx)
        .themes()
        .keys()
        .map(|k| k.to_string())
        .collect();
    names.sort();
    let mut out = vec![LINEAR_PRESET_LABEL.to_string()];
    out.extend(names);
    out
}

/// 展示名 → 存储名（Linear 之外的官方预设直接用注册名）。
pub fn preset_storage_name(label: &str) -> String {
    if label == LINEAR_PRESET_LABEL {
        DEFAULT_PRESET.to_string()
    } else {
        label.to_string()
    }
}
