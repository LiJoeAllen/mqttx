//! GPUI 界面层。

pub mod aliyun_dialog;
pub mod app;
pub mod connection_form;
pub mod connection_view;
pub mod logo;
pub mod presets_dialog;
pub mod resource_dialog;
pub mod settings_dialog;
pub mod variables_dialog;
pub mod widgets;

/// 统一使用全量 Lucide 图标（gpui-kit-assets 已随应用加载）。
pub use gpui_kit::assets::IconName;
