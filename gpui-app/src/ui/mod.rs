//! GPUI 界面层：主窗口（app）、连接工作区（connection_view）、
//! 连接表单、对话框集合与共享小组件。

pub mod app;
pub mod connection_form;
pub mod connection_view;
pub mod dialogs;
pub mod i18n;
pub mod logo;
pub mod widgets;

/// 统一使用全量 Lucide 图标（gpui-kit-assets 已随应用加载）。
pub use gpui_kit::assets::IconName;
