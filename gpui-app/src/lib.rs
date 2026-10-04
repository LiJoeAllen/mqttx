//! MQTTX：数据模型、持久化、MQTT 引擎与 GPUI 界面。

// 语言包编译期内嵌（key = 语义标识符，en/zh-CN 并排）；miss 回退 en
rust_i18n::i18n!("locales", fallback = "en");

/// 把 gpui-kit 组件内置文案（按钮/日历/空态等）并入当前语言包。
/// 启动时调用一次；extend! 展开引用本 crate 的 _rust_i18n_backend，
/// 必须在 lib 内调用（bin crate 的 crate:: 指向不同根）。
pub fn init_i18n() {
    use gpui_kit::component as gpui_component;
    rust_i18n::extend!(gpui_component);
}

pub mod aliyun;
pub mod model;
pub mod mqtt;
pub mod platform;
pub mod store;
pub mod sysmon;
pub mod ui;
pub mod update;
