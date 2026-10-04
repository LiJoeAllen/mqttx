//! 单个连接的工作区：订阅列表 + 消息流 + 发布面板 + 日志。
//!
//! - [`subscribe`] 订阅表单/列表的操作与渲染
//! - [`messages`] 顶栏过滤、消息流与日志渲染
//! - [`publish`] 发布表单/预设的操作与渲染
//! - [`vars`] 模板变量面板
//! - [`util`] topic 匹配、格式化等纯函数
use std::sync::Arc;

use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::select::SelectState;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme as _, Disableable as _,
    Selectable as _, Sizable as _, StyledExt as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    px, App, AppContext as _, Context, Entity,
    InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription as InputSubscription, Window,
};

use crate::model::ConnectionConfig;
use crate::ui::i18n;
use crate::mqtt::MqttEngine;
use crate::ui::app::MqttXApp;
use crate::ui::widgets::{make_select, KvEditor, OptionDelegate};

mod messages;
mod publish;
mod subscribe;
mod util;
mod vars;
use util::*;

const QOS: [&str; 3] = ["QoS 0", "QoS 1", "QoS 2"];
const PAYLOAD_FORMATS: [&str; 4] = ["Plaintext", "JSON", "Base64", "Hex"];
/// Retain Handling 0/1/2 的下拉文案
fn retain_handling_labels() -> [&'static str; 3] {
    [i18n::t("sub.rh0"), i18n::t("sub.rh1"), i18n::t("sub.rh2")]
}
const MAX_RENDERED_MESSAGES: usize = 300;
/// 日志渲染上限，与 app.rs 的 MAX_LOGS 保持一致
const MAX_RENDERED_LOGS: usize = 3000;
/// 订阅色板预设：色相（度）+ 名称
const PRESET_HUES: [(f32, &str); 10] = [
    (0., "color.red"),
    (30., "color.orange"),
    (60., "color.yellow"),
    (120., "color.green"),
    (160., "color.cyan"),
    (200., "color.blue"),
    (240., "color.indigo"),
    (270., "color.purple"),
    (300., "color.magenta"),
    (330., "color.rose"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Messages,
    Logs,
}

/// 消息流方向过滤（与搜索、订阅过滤叠加生效）
#[derive(Clone, Copy, PartialEq, Eq)]
enum DirFilter {
    All,
    Received,
    Published,
}

impl DirFilter {
    /// 返回 i18n 语义 key；Hex/Base64 为语言中性字面量。
    fn label(self) -> &'static str {
        match self {
            Self::All => "common.all",
            Self::Received => "dir.received",
            Self::Published => "dir.published",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::All => 0,
            Self::Received => 1,
            Self::Published => 2,
        }
    }

    fn from_index(i: usize) -> Self {
        match i {
            1 => Self::Received,
            2 => Self::Published,
            _ => Self::All,
        }
    }
}

/// 展开消息详情里的 payload 展示格式。任一时刻只展开一条消息，
/// 视图级单值即可视作「逐消息」选择。
#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailFormat {
    Auto,
    Text,
    Hex,
    Base64,
}

impl DetailFormat {
    /// 返回 i18n 语义 key；Hex/Base64 为语言中性字面量。
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "common.auto",
            Self::Text => "common.text",
            Self::Hex => "Hex",
            Self::Base64 => "Base64",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Auto => 0,
            Self::Text => 1,
            Self::Hex => 2,
            Self::Base64 => 3,
        }
    }

    fn from_index(i: usize) -> Self {
        match i {
            1 => Self::Text,
            2 => Self::Hex,
            3 => Self::Base64,
            _ => Self::Auto,
        }
    }
}

pub struct ConnectionView {
    conn_id: String,
    app: gpui_kit::WeakEntity<MqttXApp>,
    engine: Arc<MqttEngine>,

    // ── 订阅 ──
    sub_topic: Entity<InputState>,
    sub_qos: Entity<SelectState<OptionDelegate>>,
    sub_alias: Entity<InputState>,
    // v5 高级订阅选项
    sub_identifier: Entity<InputState>,
    sub_no_local: bool,
    sub_rap: bool,
    sub_retain_handling: Entity<SelectState<OptionDelegate>>,
    sub_show_advanced: bool,
    /// 编辑中的旧主题：提交时用旧主题移除原订阅，避免改名过程丢数据
    editing: Option<String>,
    /// 点击订阅项激活的消息过滤主题（再点一次取消）
    sub_filter: Option<String>,

    // 发布
    pub_topic: Entity<InputState>,
    payload: Entity<TextareaState>,
    pub_qos: Entity<SelectState<OptionDelegate>>,
    payload_format: Entity<SelectState<OptionDelegate>>,
    retain: bool,
    content_type: Entity<InputState>,
    user_props: Entity<KvEditor>,
    msg_expiry: Entity<InputState>,
    response_topic: Entity<InputState>,
    correlation_data: Entity<InputState>,
    show_props: bool,

    // 过滤与面板
    filter: Entity<InputState>,
    panel: Panel,
    expanded: Option<u64>,
    /// 当前 detail_format 归属的消息：切换到另一条消息时据此重置格式，
    /// 同一条内折叠/展开则保留用户选择
    detail_owner: Option<u64>,
    msg_dir: DirFilter,
    detail_format: DetailFormat,
    /// 消息流暂停滚动：消息照常入环形缓冲，仅不再触发本视图重绘
    paused: bool,
    /// 订阅面板抽屉收起状态（会话内有效）
    subs_collapsed: bool,
    /// 底部发布栏抽屉收起状态（会话内有效）
    publish_collapsed: bool,

    // 预设
    preset_name: Entity<InputState>,

    // 发布面板内联变量绑定
    /// 「变量」小节展开状态（仅在提取到占位符时渲染整节）
    show_vars: bool,
    /// 提取到的 `{{key}}` → 值输入框与变更订阅，按提取顺序排列
    var_rows: Vec<(String, Entity<InputState>, InputSubscription)>,
}

impl ConnectionView {
    pub fn new(
        conn_id: String,
        app: gpui_kit::WeakEntity<MqttXApp>,
        engine: std::sync::Arc<MqttEngine>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {

        let sub_topic = cx.new(|cx| {
            InputState::new(window, cx).placeholder(i18n::t("sub.topic_placeholder"))
        });
        let sub_qos = make_select(&QOS, 0, window, cx);
        let sub_alias = cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t("sub.alias")));
        let sub_identifier = cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t("sub.identifier")));
        let sub_retain_handling = make_select(&retain_handling_labels(), 0, window, cx);
        let pub_topic = cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t("pub.topic_placeholder")));
        let payload = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(i18n::t("pub.payload_placeholder"))
        });
        let pub_qos = make_select(&QOS, 0, window, cx);
        let payload_format = make_select(&PAYLOAD_FORMATS, 0, window, cx);
        let content_type = cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t("pub.content_type_ph")));
        let msg_expiry = cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t("pub.seconds_hint")));
        let response_topic =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t("pub.response_topic_ph")));
        let correlation_data =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t("pub.correlation_ph")));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t("msg.filter_placeholder")));
        let user_props = cx.new(|cx| KvEditor::new("pub", &[], window, cx));
        let preset_name =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t("pub.preset_name_hint")));

        Self {
            conn_id,
            app,
            engine,
            sub_topic,
            sub_qos,
            sub_alias,
            sub_identifier,
            sub_no_local: false,
            sub_rap: false,
            sub_retain_handling,
            sub_show_advanced: false,
            editing: None,
            sub_filter: None,
            pub_topic,
            payload,
            pub_qos,
            payload_format,
            retain: false,
            content_type,
            user_props,
            msg_expiry,
            response_topic,
            correlation_data,
            show_props: false,
            filter,
            panel: Panel::Messages,
            expanded: None,
            detail_owner: None,
            msg_dir: DirFilter::All,
            detail_format: DetailFormat::Auto,
            paused: false,
            subs_collapsed: false,
            publish_collapsed: false,
            preset_name,
            show_vars: true,
            var_rows: Vec::new(),
        }
    }

    fn with_app<R>(&self, cx: &App, f: impl FnOnce(&MqttXApp) -> R) -> Option<R> {
        self.app.upgrade().map(|a| f(a.read(cx)))
    }

    fn config(&self, cx: &App) -> Option<ConnectionConfig> {
        let id = self.conn_id.clone();
        self.with_app(cx, |app| {
            app.connections.iter().find(|c| c.id == id).cloned()
        })
        .flatten()
    }

    fn is_v5(&self, cx: &App) -> bool {
        self.config(cx).map(|c| c.protocol.is_v5()).unwrap_or(true)
    }

    /// 消息流是否处于暂停滚动状态（引擎事件泵据此决定是否刷新本视图）。
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Ctrl+F：把焦点移到消息过滤框（键盘直达过滤入口）。
    pub fn focus_filter(&self, window: &mut Window, cx: &mut App) {
        let handle = gpui_kit::component::input::AnyInputState::from(self.filter.clone())
            .focus_handle(cx);
        window.focus(&handle, cx);
    }


}

impl Render for ConnectionView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = self.panel;
        v_flex()
            .size_full()
            .min_w(px(0.))
            .child(self.render_top_bar(cx))
            .child(self.render_subscribe_bar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    // 订阅面板抽屉：收起时只剩窄 rail
                    .child(if self.subs_collapsed {
                        self.render_subs_rail(cx).into_any_element()
                    } else {
                        self.render_subscriptions(cx).into_any_element()
                    })
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .h_full()
                            .child(
                                TabBar::new("workspace-tabs")
                                    .segmented()
                                    .selected_index(if panel == Panel::Messages { 0 } else { 1 })
                                    .child(
                                        Tab::new()
                                            .label(i18n::t("connview.messages"))
                                            .selected(panel == Panel::Messages)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.panel = Panel::Messages;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Tab::new()
                                            .label(i18n::t("connview.logs"))
                                            .selected(panel == Panel::Logs)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.panel = Panel::Logs;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(match panel {
                                Panel::Messages => self.render_messages(cx).into_any_element(),
                                Panel::Logs => self.render_logs(cx).into_any_element(),
                            }),
                    ),
            )
            .child(if self.publish_collapsed {
                self.render_publish_rail(cx).into_any_element()
            } else {
                self.render_publish_bar(cx).into_any_element()
            })
    }
}

