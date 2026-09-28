//! 单个连接的工作区：订阅列表 + 消息流 + 发布面板 + 日志。

use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    h_flex, notification::Notification, v_flex, ActiveTheme as _, Disableable as _,
    Selectable as _, Sizable as _, StyledExt as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use crate::ui::IconName;
use gpui_kit::{
    div, hsla, px, App, AppContext as _, ClipboardItem, Context, Entity, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription as InputSubscription, Window,
};
use gpui_kit::component::IndexPath;

use crate::model::{
    render_template, render_will_templates, ConnectionConfig, ConnectionStatus, Direction, GlobalVariable,
    LogLevel, MqttRecord, PayloadFormat, PublishParams, SubscribeOptions, Subscription,
};
use crate::mqtt::MqttEngine;
use crate::ui::app::MqttXApp;
use crate::ui::widgets::{field, format_time, make_select, KvEditor, OptionDelegate};

const QOS: [&str; 3] = ["QoS 0", "QoS 1", "QoS 2"];
const PAYLOAD_FORMATS: [&str; 4] = ["Plaintext", "JSON", "Base64", "Hex"];
/// Retain Handling 0/1/2 的下拉文案
const RETAIN_HANDLING: [&str; 3] = ["0 每次发送", "1 仅新订阅", "2 不发送"];
const MAX_RENDERED_MESSAGES: usize = 300;
/// 日志渲染上限，与 app.rs 的 MAX_LOGS 保持一致
const MAX_RENDERED_LOGS: usize = 3000;
/// 订阅色板预设：色相（度）+ 名称
const PRESET_HUES: [(f32, &str); 10] = [
    (0., "红"),
    (30., "橙"),
    (60., "黄"),
    (120., "绿"),
    (160., "青"),
    (200., "蓝"),
    (240., "靛"),
    (270., "紫"),
    (300., "品红"),
    (330., "玫红"),
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
    fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Received => "接收",
            Self::Published => "发布",
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
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "自动",
            Self::Text => "文本",
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
            InputState::new(window, cx).placeholder("订阅主题，支持 # +，多个用逗号/空格分隔")
        });
        let sub_qos = make_select(&QOS, 0, window, cx);
        let sub_alias = cx.new(|cx| InputState::new(window, cx).placeholder("别名（可选）"));
        let sub_identifier = cx.new(|cx| InputState::new(window, cx).placeholder("订阅标识符"));
        let sub_retain_handling = make_select(&RETAIN_HANDLING, 0, window, cx);
        let pub_topic = cx.new(|cx| InputState::new(window, cx).placeholder("发布主题"));
        let payload = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("输入消息负载，支持 {{变量名}} 与 {{$ts}} {{$uuid}}")
        });
        let pub_qos = make_select(&QOS, 0, window, cx);
        let payload_format = make_select(&PAYLOAD_FORMATS, 0, window, cx);
        let content_type = cx.new(|cx| InputState::new(window, cx).placeholder("Content-Type（可选）"));
        let msg_expiry = cx.new(|cx| InputState::new(window, cx).placeholder("秒，如 60"));
        let response_topic =
            cx.new(|cx| InputState::new(window, cx).placeholder("Response Topic（可选）"));
        let correlation_data =
            cx.new(|cx| InputState::new(window, cx).placeholder("Correlation Data（可选）"));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("过滤主题或内容…"));
        let user_props = cx.new(|cx| KvEditor::new("pub", &[], window, cx));
        let preset_name =
            cx.new(|cx| InputState::new(window, cx).placeholder("预设名称，如 温度上报"));

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

    // ── 订阅数据操作 ────────────────────────────────────────────────────────

    /// 订阅按 (连接, 主题) 唯一：先摘掉旧条目再走 `add_subscription`，
    /// 借它入列 + `save_subscriptions` 落盘的逻辑完成 upsert（storage 字段是私有的）。
    /// 落盘顺序以 add 时为准，入列后移回原位以保持界面顺序稳定。
    fn upsert_subscription(&self, sub: Subscription, cx: &mut Context<Self>) {
        let Some(app_entity) = self.app.upgrade() else {
            return;
        };
        app_entity.update(cx, |app, cx| {
            let slot = app
                .subscriptions
                .iter()
                .position(|s| s.connection_id == sub.connection_id && s.topic == sub.topic);
            app.subscriptions.retain(|s| {
                !(s.connection_id == sub.connection_id && s.topic == sub.topic)
            });
            let new_id = sub.id.clone();
            app.add_subscription(sub);
            if let Some(pos) = slot
                && let Some(from) = app.subscriptions.iter().position(|s| s.id == new_id)
            {
                let moved = app.subscriptions.remove(from);
                app.subscriptions.insert(pos.min(app.subscriptions.len()), moved);
            }
            cx.notify();
        });
        cx.notify();
    }

    fn set_subscription_color(&mut self, sub_id: &str, hue: Option<f32>, cx: &mut Context<Self>) {
        let sub = self
            .with_app(cx, |app| {
                app.subscriptions
                    .iter()
                    .find(|s| s.id == sub_id)
                    .cloned()
            })
            .flatten();
        if let Some(mut sub) = sub {
            sub.color = hue;
            self.upsert_subscription(sub, cx);
        }
    }

    /// 启停订阅：置灰/恢复行显示，并在已连接时对引擎退订/重订。
    fn toggle_subscription_enabled(&mut self, sub_id: &str, cx: &mut Context<Self>) {
        let Some(mut sub) = self
            .with_app(cx, |app| {
                app.subscriptions
                    .iter()
                    .find(|s| s.id == sub_id)
                    .cloned()
            })
            .flatten()
        else {
            return;
        };
        sub.enabled = !sub.enabled;
        let enabled = sub.enabled;
        let topic = sub.topic.clone();
        // 停用的订阅不再参与消息过滤，指向它的过滤一并清掉
        if !enabled && self.sub_filter.as_deref() == Some(topic.as_str()) {
            self.sub_filter = None;
        }
        let conn = self.conn_id.clone();
        self.upsert_subscription(sub.clone(), cx);
        if self.engine.is_connected(&conn) {
            if enabled {
                self.engine
                    .subscribe_with_options(conn, sub.clone(), SubscribeOptions::from(&sub));
            } else {
                self.engine.unsubscribe(conn, topic);
            }
        }
    }

    /// 回填输入区进入编辑态；旧订阅保留到提交时才替换，中途放弃不丢数据。
    fn start_edit_subscription(&mut self, sub: &Subscription, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(sub.topic.clone());
        self.sub_topic
            .update(cx, |s, cx| s.set_value(sub.topic.clone(), window, cx));
        let qos_idx = (sub.qos.min(2)) as usize;
        self.sub_qos.update(cx, |s, cx| {
            s.set_selected_index(Some(IndexPath::new(qos_idx)), window, cx)
        });
        let alias = sub.alias.clone().unwrap_or_default();
        self.sub_alias
            .update(cx, |s, cx| s.set_value(alias, window, cx));
        let ident = sub
            .sub_identifier
            .map(|v| v.to_string())
            .unwrap_or_default();
        self.sub_identifier
            .update(cx, |s, cx| s.set_value(ident, window, cx));
        self.sub_no_local = sub.no_local;
        self.sub_rap = sub.retain_as_published;
        let rh_idx = sub.retain_handling.min(2) as usize;
        self.sub_retain_handling.update(cx, |s, cx| {
            s.set_selected_index(Some(IndexPath::new(rh_idx)), window, cx)
        });
        self.sub_show_advanced = true;
        cx.notify();
    }

    /// 重置订阅表单（主题/别名/高级字段）：新增提交与取消编辑共用，
    /// 避免高级选项残留到下一次订阅。
    fn reset_subscribe_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sub_topic
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.sub_alias
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.sub_identifier
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.sub_no_local = false;
        self.sub_rap = false;
        self.sub_retain_handling.update(cx, |s, cx| {
            s.set_selected_index(Some(IndexPath::new(0)), window, cx)
        });
    }

    // ── 订阅 / 发布动作 ─────────────────────────────────────────────────────

    fn do_subscribe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.sub_topic.read(cx).value().to_string();
        let topics = split_topic_list(&raw);
        if topics.is_empty() {
            window.push_notification(Notification::warning("订阅主题不能为空"), cx);
            return;
        }
        let qos = self.sub_qos.read(cx).selected_value().copied().unwrap_or(0) as u8;
        let alias_raw = self.sub_alias.read(cx).value().to_string();
        let alias = {
            let t = alias_raw.trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        let is_v5 = self.is_v5(cx);
        // v5 选项只对 v5 连接生效，v4 下不读取（避免无效输入阻断订阅）
        let mut sub_identifier = None;
        if is_v5 {
            let ident = self.sub_identifier.read(cx).value().to_string();
            let t = ident.trim();
            if !t.is_empty() {
                match t.parse::<u32>() {
                    Ok(v) if v > 0 => sub_identifier = Some(v),
                    _ => {
                        window.push_notification(
                            Notification::warning("订阅标识符须为正整数"),
                            cx,
                        );
                        return;
                    }
                }
            }
        }
        let no_local = self.sub_no_local;
        let rap = self.sub_rap;
        let retain_handling = self
            .sub_retain_handling
            .read(cx)
            .selected_value()
            .copied()
            .unwrap_or(0) as u8;

        let connected = self.engine.is_connected(&self.conn_id);
        let conn = self.conn_id.clone();

        // 记录编辑来源：编辑态提交后以目标订阅回填，保住编辑上下文
        let was_editing = self.editing.clone();
        // 改名编辑时旧记录随后会被删除，先取出供新主题承接 id/color/enabled
        let editing_old = was_editing.as_ref().and_then(|old_topic| {
            self.with_app(cx, |app| {
                app.subscriptions
                    .iter()
                    .find(|s| s.connection_id == conn && s.topic == *old_topic)
                    .cloned()
            })
            .flatten()
        });

        // 编辑提交：旧主题不在新列表里时先移除（含引擎退订），新主题由下方 upsert 覆盖
        if let Some(old_topic) = self.editing.take()
            && !topics.contains(&old_topic)
        {
            if self.sub_filter.as_deref() == Some(old_topic.as_str()) {
                self.sub_filter = None;
            }
            if let Some(app_entity) = self.app.upgrade() {
                app_entity.update(cx, |app, cx| {
                    app.remove_subscription(&conn, &old_topic);
                    cx.notify();
                });
            }
        }

        for topic in &topics {
            // 按 (连接, 主题) 查旧记录：继承 id/color/enabled，避免重复提交把备注色清掉、
            // 把停用中的订阅静默重启
            let existing = self
                .with_app(cx, |app| {
                    app.subscriptions
                        .iter()
                        .find(|s| s.connection_id == conn && s.topic == *topic)
                        .cloned()
                })
                .flatten();
            let sub = match existing {
                Some(mut old) => {
                    old.qos = qos;
                    old.alias = alias.clone();
                    if is_v5 {
                        old.sub_identifier = sub_identifier;
                        old.no_local = no_local;
                        old.retain_as_published = rap;
                        old.retain_handling = retain_handling;
                    }
                    old
                }
                None => {
                    // 改名编辑的单主题提交：新主题承接旧记录身份与备注，避免改名丢数据
                    if topics.len() == 1 && let Some(mut old) = editing_old.clone() {
                        old.topic = topic.clone();
                        old.qos = qos;
                        old.alias = alias.clone();
                        if is_v5 {
                            old.sub_identifier = sub_identifier;
                            old.no_local = no_local;
                            old.retain_as_published = rap;
                            old.retain_handling = retain_handling;
                        }
                        old
                    } else {
                        let mut sub = Subscription::new(conn.clone(), topic.clone(), qos);
                        sub.alias = alias.clone();
                        if is_v5 {
                            sub.sub_identifier = sub_identifier;
                            sub.no_local = no_local;
                            sub.retain_as_published = rap;
                            sub.retain_handling = retain_handling;
                        }
                        sub
                    }
                }
            };
            // 继承到的停用订阅保持退订状态，不因重新提交被拉起
            let keep_disabled = !sub.enabled;
            self.upsert_subscription(sub.clone(), cx);
            if connected && !keep_disabled {
                self.engine.subscribe_with_options(
                    conn.clone(),
                    sub.clone(),
                    SubscribeOptions::from(&sub),
                );
            }
        }
        if !connected {
            let hint = if self
                .config(cx)
                .map(|c| c.auto_resubscribe)
                .unwrap_or(true)
            {
                "未连接：订阅已保存，连接后自动恢复"
            } else {
                "未连接：订阅已保存"
            };
            window.push_notification(Notification::warning(hint), cx);
        }

        // 编辑态：以刚保存的目标订阅回填表单继续保留上下文；
        // 新增态：清空主题/别名并重置高级字段，避免残留到下一次订阅
        if was_editing.is_some() && topics.len() == 1 {
            let target = topics[0].clone();
            let saved = self
                .with_app(cx, |app| {
                    app.subscriptions
                        .iter()
                        .find(|s| s.connection_id == conn && s.topic == target)
                        .cloned()
                })
                .flatten();
            if let Some(saved) = saved {
                self.start_edit_subscription(&saved, window, cx);
            } else {
                self.editing = None;
                self.reset_subscribe_form(window, cx);
            }
        } else {
            self.editing = None;
            self.reset_subscribe_form(window, cx);
        }
        cx.notify();
    }

    fn do_publish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 未连接时消息发不出去，先拦截给出与订阅路径一致的提示
        if !self.engine.is_connected(&self.conn_id) {
            window.push_notification(Notification::warning("未连接：消息未发送"), cx);
            return;
        }
        let mut params = self.collect_publish_params(cx);
        if params.topic.trim().is_empty() {
            window.push_notification(Notification::warning("发布主题不能为空"), cx);
            return;
        }
        // 过期秒数输入非法时 collect 会静默得到 None，这里显式拦截避免误发
        let expiry_raw = self.msg_expiry.read(cx).value().to_string();
        if !expiry_raw.trim().is_empty() && params.message_expiry_interval.is_none() {
            window.push_notification(Notification::warning("消息过期须为非负整数（秒）"), cx);
            return;
        }

        let vars = self.with_app(cx, |app| app.variables.clone()).unwrap_or_default();
        params.topic = render_template(&params.topic, &vars);
        params.payload = render_template(&params.payload, &vars);
        // {{topic}} 等变量渲染后可能变空串，渲染完成后再校验一次
        if params.topic.trim().is_empty() {
            window.push_notification(Notification::warning("发布主题不能为空"), cx);
            return;
        }
        // {{变量}} 同样作用于用户属性的 key/value
        params.user_properties = params
            .user_properties
            .into_iter()
            .map(|(k, v)| (render_template(&k, &vars), render_template(&v, &vars)))
            .collect();
        // v5 文本属性（Content-Type / Response Topic / Correlation Data）一并注入
        params.content_type = params.content_type.map(|v| render_template(&v, &vars));
        params.response_topic = params.response_topic.map(|v| render_template(&v, &vars));
        params.correlation_data = params.correlation_data.map(|v| render_template(&v, &vars));

        if let PayloadFormat::Json = params.payload_format
            && let Err(e) = serde_json::from_str::<serde_json::Value>(&params.payload) {
                window.push_notification(
                    Notification::warning(format!("JSON 格式无效: {e}")),
                    cx,
                );
                return;
            }
        match params.payload_format.encode(&params.payload) {
            Ok(bytes) => params.raw_bytes = Some(bytes),
            Err(e) => {
                window.push_notification(Notification::warning(e), cx);
                return;
            }
        }

        // v3.1.1 忽略 v5 属性，避免残留输入带进发布报文
        if !self.is_v5(cx) {
            params.user_properties.clear();
            params.content_type = None;
            params.message_expiry_interval = None;
            params.response_topic = None;
            params.correlation_data = None;
        }
        self.engine.publish(self.conn_id.clone(), params);
    }

    /// 读取当前发布面板上的参数（模板渲染前的原始值）；保存预设与发布共用，
    /// 须覆盖全部 v5 属性与用户属性。
    fn collect_publish_params(&self, cx: &App) -> PublishParams {
        let qos = self.pub_qos.read(cx).selected_value().copied().unwrap_or(0) as u8;
        let format = PayloadFormat::ALL
            [self.payload_format.read(cx).selected_value().copied().unwrap_or(0)];
        let opt = |s: String| {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        PublishParams {
            topic: self.pub_topic.read(cx).value().to_string(),
            payload: self.payload.read(cx).value().to_string(),
            payload_format: format,
            qos,
            retain: self.retain,
            user_properties: self.user_props.read(cx).pairs(cx),
            content_type: opt(self.content_type.read(cx).value().to_string()),
            message_expiry_interval: {
                let t = self.msg_expiry.read(cx).value().to_string();
                let t = t.trim();
                if t.is_empty() { None } else { t.parse::<u32>().ok() }
            },
            response_topic: opt(self.response_topic.read(cx).value().to_string()),
            correlation_data: opt(self.correlation_data.read(cx).value().to_string()),
            ..Default::default()
        }
    }

    /// 把预设应用到发布面板控件。
    fn apply_publish_preset(
        &mut self,
        params: &PublishParams,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pub_topic
            .update(cx, |s, cx| s.set_value(params.topic.clone(), window, cx));
        self.payload
            .update(cx, |s, cx| s.set_value(params.payload.clone(), window, cx));
        // 预设未指定 Content-Type 时清空旧值，避免上一次输入残留
        let ct = params.content_type.clone().unwrap_or_default();
        self.content_type
            .update(cx, |s, cx| s.set_value(ct, window, cx));
        let expiry = params
            .message_expiry_interval
            .map(|v| v.to_string())
            .unwrap_or_default();
        self.msg_expiry
            .update(cx, |s, cx| s.set_value(expiry, window, cx));
        let rt = params.response_topic.clone().unwrap_or_default();
        self.response_topic
            .update(cx, |s, cx| s.set_value(rt, window, cx));
        let cd = params.correlation_data.clone().unwrap_or_default();
        self.correlation_data
            .update(cx, |s, cx| s.set_value(cd, window, cx));
        // KvEditor 不支持运行时整体替换，直接重建实体回填用户属性
        self.user_props = cx.new(|cx| KvEditor::new("pub", &params.user_properties, window, cx));
        let qos_idx = params.qos.min(2) as usize;
        self.pub_qos.update(cx, |s, cx| {
            s.set_selected_index(Some(IndexPath::new(qos_idx)), window, cx)
        });
        let fmt_idx = PayloadFormat::ALL
            .iter()
            .position(|f| *f == params.payload_format)
            .unwrap_or(0);
        self.payload_format.update(cx, |s, cx| {
            s.set_selected_index(Some(IndexPath::new(fmt_idx)), window, cx)
        });
        self.retain = params.retain;
        cx.notify();
    }
}

// ── 渲染 ──────────────────────────────────────────────────────────────────────

impl ConnectionView {
    fn render_top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.conn_id.clone();
        let (name, status, is_v5, error) = self
            .with_app(cx, |app| {
                let cfg = app.connections.iter().find(|c| c.id == id);
                (
                    cfg.map(|c| c.name.clone()).unwrap_or_else(|| id.clone()),
                    app.statuses.get(&id).copied().unwrap_or(ConnectionStatus::Disconnected),
                    cfg.map(|c| c.protocol.is_v5()).unwrap_or(true),
                    app.errors.get(&id).cloned(),
                )
            })
            .unwrap_or((id.clone(), ConnectionStatus::Disconnected, true, None));
        let connected = matches!(status, ConnectionStatus::Connected);
        let connecting = matches!(status, ConnectionStatus::Connecting);
        let _ = is_v5;

        let status_text = status.label();
        let status_tone = match status {
            ConnectionStatus::Connected => cx.theme().success,
            ConnectionStatus::Connecting => cx.theme().warning,
            ConnectionStatus::Error => cx.theme().danger,
            ConnectionStatus::Disconnected => cx.theme().muted_foreground,
        };
        // Error 状态把失败原因展示出来，避免只有「错误」二字无从排查
        let error_hint = matches!(status, ConnectionStatus::Error)
            .then(|| error.clone())
            .flatten();

        h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .px_3()
            .h_12()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().size(px(8.)).rounded_full().bg(status_tone))
            .child(div().text_sm().font_semibold().child(name))
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .px_1p5()
                    .py_0p5()
                    .rounded_md()
                    .bg(status_tone.alpha(0.12))
                    .child(div().size(px(5.)).rounded_full().bg(status_tone))
                    .child(
                        div()
                            .text_xs()
                            .text_color(status_tone)
                            .child(status_text),
                    ),
            )
            // Error 副文本展示失败原因（截断避免挤掉按钮）
            .when_some(error_hint.clone(), |h, e| {
                h.child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(e),
                )
            })
            .when(error_hint.is_none(), |h| h.child(div().flex_1()))
            .child(
                Button::new(SharedString::from(format!("conn-toggle-{}", self.conn_id)))
                    .icon(if connected {
                        IconName::Square
                    } else {
                        IconName::PlugZap
                    })
                    .label(if connected {
                        "断开"
                    } else if connecting {
                        "连接中…"
                    } else {
                        "连接"
                    })
                    .when(connected, |b| b.danger().ghost())
                    .when(!connected, |b| b.primary().ghost())
                    // Connecting 期间禁用，避免重复点击重启连接流程
                    .disabled(connecting)
                    .small()
                    .when_some(error_hint.clone(), |b, e| b.tooltip(e))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        // Connecting 期间（按钮禁用之外再兜一层）不响应，防止重复 connect 重启流程
                        let connecting = this
                            .with_app(cx, |app| {
                                app.statuses.get(&this.conn_id)
                                    == Some(&ConnectionStatus::Connecting)
                            })
                            .unwrap_or(false);
                        if connecting {
                            return;
                        }
                        let cfg = this.config(cx);
                        if let Some(mut cfg) = cfg {
                            if this.engine.is_connected(&cfg.id) {
                                this.engine.close(&cfg.id, true);
                            } else {
                                // 遗嘱 {{变量}} 在每次连接时按当前全局变量求值
                                let vars = this
                                    .with_app(cx, |app| app.variables.clone())
                                    .unwrap_or_default();
                                render_will_templates(&mut cfg, &vars);
                                this.engine.connect(cfg);
                            }
                        }
                    })),
            )
    }

    fn render_subscribe_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let sub_topic = self.sub_topic.clone();
        let sub_qos = self.sub_qos.clone();
        let sub_alias = self.sub_alias.clone();
        let is_v5 = self.is_v5(cx);
        let editing_topic = self.editing.clone();
        let editing = editing_topic.is_some();
        let show_advanced = self.sub_show_advanced && is_v5;

        let mut bar = v_flex()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary);
        bar = bar.child(
            h_flex()
                .gap_2()
                .px_3()
                .h_12()
                .items_center()
                // 编辑态显式提示目标主题，避免被误当成新增提交
                .when_some(editing_topic.clone(), |h, topic| {
                    h.child(
                        div()
                            .px_1p5()
                            .py_0p5()
                            .rounded_md()
                            .bg(cx.theme().warning.alpha(0.15))
                            .text_xs()
                            .text_color(cx.theme().warning)
                            .child(format!("正在编辑：{topic}")),
                    )
                })
                .child(div().flex_1().min_w(px(0.)).child(Input::new(&sub_topic).small()))
                .child(div().w(px(96.)).child(Select::new(&sub_qos).small()))
                .child(div().w(px(112.)).child(Input::new(&sub_alias).small()))
                .when(is_v5, |h| {
                    h.child(
                        Button::new(SharedString::from(format!("sub-adv-{}", self.conn_id)))
                            .icon(IconName::SlidersHorizontal)
                            .label("高级")
                            .ghost()
                            .small()
                            .when(show_advanced, |b| b.selected(true))
                            .tooltip("MQTT 5 订阅选项")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sub_show_advanced = !this.sub_show_advanced;
                                cx.notify();
                            })),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!("subscribe-{}", self.conn_id)))
                        .icon(if editing { IconName::Check } else { IconName::Plus })
                        .label(if editing { "更新" } else { "订阅" })
                        // 编辑用 warning 色与新增的 primary 区分
                        .when(editing, |b| b.warning())
                        .when(!editing, |b| b.primary())
                        .small()
                        .on_click(cx.listener(|this, _, window, cx| this.do_subscribe(window, cx))),
                )
                .when(editing, |h| {
                    h.child(
                        Button::new(SharedString::from(format!("sub-edit-cancel-{}", self.conn_id)))
                            .icon(IconName::Close)
                            .ghost()
                            .small()
                            .tooltip("取消编辑")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.editing = None;
                                // 清空回填到输入框的主题/别名/高级字段，回到新增态
                                this.reset_subscribe_form(window, cx);
                                cx.notify();
                            })),
                    )
                }),
        );

        if show_advanced {
            let sub_identifier = self.sub_identifier.clone();
            let sub_no_local = self.sub_no_local;
            let sub_rap = self.sub_rap;
            let sub_retain_handling = self.sub_retain_handling.clone();
            bar = bar.child(
                h_flex()
                    .gap_4()
                    .px_3()
                    .pb_2()
                    .items_start()
                    .child(
                        div()
                            .w(px(150.))
                            .child(field("订阅标识符", Input::new(&sub_identifier).small())),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .pt_4()
                            .child(Switch::new("sub-no-local").checked(sub_no_local).on_change(
                                cx.listener(|this, v, _, cx| {
                                    this.sub_no_local = *v;
                                    cx.notify();
                                }),
                            ))
                            .child(div().text_xs().child("No Local")),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .pt_4()
                            .child(Switch::new("sub-rap").checked(sub_rap).on_change(
                                cx.listener(|this, v, _, cx| {
                                    this.sub_rap = *v;
                                    cx.notify();
                                }),
                            ))
                            .child(div().text_xs().child("Retain As Published")),
                    )
                    .child(
                        div().w(px(150.)).child(field(
                            "Retain Handling",
                            Select::new(&sub_retain_handling).small(),
                        )),
                    ),
            );
        }
        bar
    }

    fn render_subscriptions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hover_bg = cx.theme().muted;
        let muted_fg = cx.theme().muted_foreground;
        let conn = self.conn_id.clone();
        let subs: Vec<Subscription> = self
            .with_app(cx, |app| {
                app.subscriptions
                    .iter()
                    .filter(|s| s.connection_id == conn)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let active_filter = self.sub_filter.clone();

        let mut list = v_flex().gap_0p5();
        if subs.is_empty() {
            list = list.child(
                v_flex()
                    .p_3()
                    .gap_2()
                    .items_center()
                    .child(
                        gpui_kit::component::Icon::new(IconName::Hash)
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .text_center()
                            .child("订阅主题后，消息会显示在右侧"),
                    ),
            );
        }
        for sub in subs {
            let enabled = sub.enabled;
            let is_active = enabled && active_filter.as_deref() == Some(sub.topic.as_str());
            let topic = sub.topic.clone();
            let sub_id = sub.id.clone();
            let alias = sub.alias.clone().filter(|a| !a.trim().is_empty());
            let primary = alias.clone().unwrap_or_else(|| topic.clone());

            // 颜色按钮：已设色显示色点，未设显示灰色「+」
            let color_btn = {
                let weak_menu = cx.weak_entity();
                let sid = sub_id.clone();
                let trigger_id = SharedString::from(format!("sub-color-{}", sub_id));
                let trigger = if let Some(hue) = sub.color {
                    Button::new(trigger_id)
                        .ghost()
                        .xsmall()
                        .tooltip("订阅颜色")
                        .child(
                            div()
                                .size(px(11.))
                                .rounded_full()
                                .bg(hue_color(hue))
                                .border_1()
                                .border_color(cx.theme().border),
                        )
                } else {
                    Button::new(trigger_id)
                        .ghost()
                        .xsmall()
                        .tooltip("订阅颜色")
                        .child(
                            div()
                                .size(px(11.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(cx.theme().muted)
                                .text_color(muted_fg)
                                .text_xs()
                                .child("+"),
                        )
                };
                trigger.dropdown_menu(move |mut menu, _, _| {
                    for (hue, name) in PRESET_HUES {
                        let weak = weak_menu.clone();
                        let sid = sid.clone();
                        let swatch = hue_color(hue);
                        // 菜单只显示颜色名，角度数值对用户没有意义
                        let label = SharedString::from(name);
                        menu = menu.item(
                            PopupMenuItem::element(move |_, _| {
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .size(px(10.))
                                            .rounded_full()
                                            .bg(swatch)
                                            .border_1()
                                            .border_color(hsla(0., 0., 1., 0.6)),
                                    )
                                    .child(div().text_sm().child(label.clone()))
                            })
                            .on_click(move |_, _, cx| {
                                weak
                                    .update(cx, |view, cx| {
                                        view.set_subscription_color(&sid, Some(hue), cx);
                                    })
                                    .ok();
                            }),
                        );
                    }
                    menu = menu.separator();
                    let weak_rand = weak_menu.clone();
                    let sid_rand = sid.clone();
                    menu = menu.item(PopupMenuItem::new("随机颜色").on_click(move |_, _, cx| {
                        let hue = (uuid::Uuid::new_v4().as_u128() % 360) as f32;
                        weak_rand
                            .update(cx, |view, cx| {
                                view.set_subscription_color(&sid_rand, Some(hue), cx);
                            })
                            .ok();
                    }));
                    let weak_clear = weak_menu.clone();
                    let sid_clear = sid.clone();
                    menu = menu.item(PopupMenuItem::new("清除颜色").on_click(move |_, _, cx| {
                        weak_clear
                            .update(cx, |view, cx| {
                                view.set_subscription_color(&sid_clear, None, cx);
                            })
                            .ok();
                    }));
                    menu
                })
            };

            let eye_sid = sub_id.clone();
            let eye_btn = Button::new(SharedString::from(format!("sub-eye-{}", sub_id)))
                .icon(if enabled { IconName::Eye } else { IconName::EyeOff })
                .ghost()
                .xsmall()
                .tooltip(if enabled { "停用订阅" } else { "启用订阅" })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_subscription_enabled(&eye_sid, cx);
                }));

            let edit_sid = sub_id.clone();
            let edit_btn = Button::new(SharedString::from(format!("sub-edit-{}", sub_id)))
                .icon(IconName::Pencil)
                .ghost()
                .xsmall()
                .tooltip("编辑订阅")
                .on_click(cx.listener(move |this, _, window, cx| {
                    let sub = this
                        .with_app(cx, |app| {
                            app.subscriptions
                                .iter()
                                .find(|s| s.id == edit_sid)
                                .cloned()
                        })
                        .flatten();
                    if let Some(sub) = sub {
                        this.start_edit_subscription(&sub, window, cx);
                    }
                }));

            let del_cid = self.conn_id.clone();
            let del_topic = topic.clone();
            let del_btn = Button::new(SharedString::from(format!("sub-del-{}", sub_id)))
                .icon(IconName::Close)
                .ghost()
                .xsmall()
                .tooltip("取消订阅")
                .on_click(cx.listener(move |this, _, _, cx| {
                    // 若删除的是当前过滤主题，一并清掉消息过滤
                    if this.sub_filter.as_deref() == Some(del_topic.as_str()) {
                        this.sub_filter = None;
                    }
                    if let Some(app) = this.app.upgrade() {
                        app.update(cx, |app, cx| {
                            app.remove_subscription(&del_cid, &del_topic);
                            cx.notify();
                        });
                    }
                    // 只有删除的是正在编辑的主题才退出编辑态，编辑别的不受影响
                    if this.editing.as_deref() == Some(del_topic.as_str()) {
                        this.editing = None;
                    }
                    cx.notify();
                }));

            // 点击区（非按钮）：切换按该主题过滤消息流；禁用的订阅不参与
            let filter_topic = topic.clone();
            let click_zone = h_flex()
                .id(SharedString::from(format!("sub-click-{}", sub_id)))
                .flex_1()
                .min_w(px(0.))
                .when(enabled, |z| {
                    z.cursor_pointer().on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.sub_filter = if this.sub_filter.as_deref()
                                == Some(filter_topic.as_str())
                            {
                                None
                            } else {
                                Some(filter_topic.clone())
                            };
                            cx.notify();
                        },
                    ))
                })
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .gap_0p5()
                        .child(
                            div()
                                .text_sm()
                                .overflow_hidden()
                                .text_ellipsis()
                                .when(!enabled, |t| t.text_color(muted_fg))
                                .child(primary),
                        )
                        .when_some(alias, |v, _| {
                            v.child(
                                div()
                                    .text_xs()
                                    .text_color(muted_fg)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(topic.clone()),
                            )
                        }),
                );

            list = list.child(
                h_flex()
                    .id(SharedString::from(format!("sub-{}-{}", self.conn_id, sub.id)))
                    .gap_1p5()
                    .items_center()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .when(is_active, |t| t.bg(cx.theme().secondary))
                    .when(!is_active, |t| t.hover(move |this| this.bg(hover_bg)))
                    .when(!enabled, |t| t.opacity(0.55))
                    .child(color_btn)
                    .child(click_zone)
                    .child(
                        div()
                            .text_xs()
                            .px_1()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_color(muted_fg)
                            .child(format!("Q{}", sub.qos)),
                    )
                    .child(eye_btn)
                    .child(edit_btn)
                    .child(del_btn),
            );
        }

        v_flex()
            .w(px(248.))
            .flex_shrink_0()
            .h_full()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().sidebar_foreground)
            .child(
                h_flex()
                    .h_11()
                    .px_3()
                    .items_center()
                    .child(div().text_sm().font_semibold().child("订阅")),
            )
            .child(div().flex_1().overflow_y_scrollbar().px_2().child(list))
    }

    fn render_message_row(
        &self,
        record: &MqttRecord,
        sub_color: Option<f32>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let seq = record.seq;
        let expanded = self.expanded == Some(seq);
        let received = matches!(record.direction, Direction::Received);
        let accent = if received {
            cx.theme().primary
        } else {
            cx.theme().success
        };
        let hover_bg = cx.theme().muted;
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let mono = cx.theme().mono_font_family.clone();
        let direction_label = if received { "接收" } else { "发布" };
        let show_millis = self
            .with_app(cx, |app| app.settings.show_millis)
            .unwrap_or(true);

        let header = h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(accent.alpha(0.14))
                    .child(
                        gpui_kit::component::Icon::new(if received {
                            IconName::ArrowDown
                        } else {
                            IconName::ArrowUp
                        })
                        .size_3()
                        .text_color(accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_sm()
                    .font_medium()
                    .overflow_hidden()
                    .text_ellipsis()
                    .font_family(mono.clone())
                    // 匹配到订阅色时主题文字着色，否则保持默认前景色
                    .when_some(sub_color, |t, hue| t.text_color(hue_color(hue)))
                    .child(record.topic.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .font_family(mono.clone())
                    .child(direction_label),
            )
            .child(
                div()
                    .text_xs()
                    .px_1()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .text_color(muted)
                    .font_family(mono.clone())
                    .child(format!("Q{}", record.qos)),
            )
            .when(record.retain, |t| {
                t.child(
                    div()
                        .text_xs()
                        .px_1()
                        .rounded_sm()
                        .bg(cx.theme().warning.alpha(0.15))
                        .text_color(cx.theme().warning)
                        .child("retain"),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .font_family(mono.clone())
                    .child(format_time(record.timestamp, show_millis)),
            );

        let mut body = v_flex()
            .flex_1()
            .min_w(px(0.))
            .px_3()
            .py_2()
            .child(header)
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .pt_0p5()
                    .font_family(mono.clone())
                    .child(preview_payload(&record.payload)),
            );

        if expanded {
            let fmt = self.detail_format;
            let detail_text = format_payload_detail(&record.payload, fmt);
            let mut details = v_flex().gap_1p5().pt_1();

            // 格式切换 + 复制按钮；stop_propagation 避免点击时又收起整行
            let fmt_group = ButtonGroup::new(SharedString::from(format!("msg-fmt-{}", seq)))
                .compact()
                .children(
                    [DetailFormat::Auto, DetailFormat::Text, DetailFormat::Hex, DetailFormat::Base64]
                        .into_iter()
                        .map(|f| {
                            Button::new(SharedString::from(format!(
                                "msg-fmt-btn-{}-{}",
                                seq,
                                f.index()
                            )))
                            .label(f.label())
                            .small()
                            .selected(f == fmt)
                        }),
                )
                .on_click(cx.listener(|this, ixs: &Vec<usize>, _, cx| {
                    cx.stop_propagation();
                    if let Some(&i) = ixs.first() {
                        this.detail_format = DetailFormat::from_index(i);
                        cx.notify();
                    }
                }));

            let copy_topic = record.topic.clone();
            let copy_payload = record.payload.clone();
            let copy_detail = details_copy_text(record, show_millis);
            let detail_toolbar = h_flex()
                .gap_1()
                .items_center()
                .child(fmt_group)
                .child(div().flex_1())
                .child(
                    Button::new(SharedString::from(format!("msg-copy-topic-{}", seq)))
                        .icon(IconName::Copy)
                        .label("主题")
                        .ghost()
                        .xsmall()
                        .tooltip("复制主题")
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_topic.clone()));
                            window.push_notification(Notification::success("已复制主题"), cx);
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("msg-copy-payload-{}", seq)))
                        .icon(IconName::Copy)
                        .label("负载")
                        .ghost()
                        .xsmall()
                        .tooltip("复制负载原文")
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_payload.clone()));
                            window.push_notification(Notification::success("已复制负载"), cx);
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("msg-copy-detail-{}", seq)))
                        .icon(IconName::Copy)
                        .label("详情")
                        .ghost()
                        .xsmall()
                        .tooltip("复制完整详情（含 v5 属性）")
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_detail.clone()));
                            window.push_notification(Notification::success("已复制详情"), cx);
                        }),
                );
            details = details.child(detail_toolbar).child(
                div()
                    .font_family(mono.clone())
                    .text_xs()
                    .p_2()
                    .rounded_md()
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(border)
                    .child(detail_text),
            );

            if !record.user_properties.is_empty() {
                let props: String = record
                    .user_properties
                    .iter()
                    .map(|(k, v)| format!("{k} = {v}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                details = details
                    .child(div().text_xs().font_semibold().child("用户属性"))
                    .child(
                        div()
                            .font_family(mono.clone())
                            .text_xs()
                            .child(props),
                    );
            }
            for (label, value) in [
                ("Content-Type", record.content_type.clone()),
                ("Response Topic", record.response_topic.clone()),
                ("Correlation Data", record.correlation_data.clone()),
                (
                    "消息过期",
                    record
                        .message_expiry_interval
                        .map(|v| format!("{v} 秒")),
                ),
                (
                    "订阅标识符",
                    record.subscription_identifier.map(|v| v.to_string()),
                ),
            ] {
                if let Some(v) = value {
                    details = details.child(
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .child(div().text_color(muted).child(format!("{label}:")))
                            .child(div().font_family(mono.clone()).child(v)),
                    );
                }
            }
            body = body.child(
                div()
                    .id(SharedString::from(format!("msg-detail-{}", seq)))
                    .mt_1()
                    .p_2()
                    .rounded_lg()
                    .bg(cx.theme().muted)
                    // 详情区点击不冒泡到行折叠，保证文本可选中
                    .on_click(|_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(details),
            );
        }

        h_flex()
            .id(SharedString::from(format!(
                "msg-{}-{}",
                record.connection_id, record.seq
            )))
            .w_full()
            .border_b_1()
            .border_color(border)
            .hover(move |this| this.bg(hover_bg))
            .cursor_pointer()
            // 左侧色条：有订阅色时显示，否则透明以保持行内对齐
            .child(
                div()
                    .w(px(3.))
                    .self_stretch()
                    .rounded_full()
                    .bg(sub_color.map(hue_color).unwrap_or(hsla(0., 0., 0., 0.))),
            )
            .child(body)
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.expanded == Some(seq) {
                    this.expanded = None;
                } else {
                    // 切换到另一条消息时格式回到 Auto，单条内折叠/展开仍记忆选择
                    if this.detail_owner != Some(seq) {
                        this.detail_format = DetailFormat::Auto;
                        this.detail_owner = Some(seq);
                    }
                    this.expanded = Some(seq);
                }
                cx.notify();
            }))
    }

    fn render_messages(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.filter.read(cx).value().to_lowercase();
        let conn = self.conn_id.clone();
        let dir = self.msg_dir;
        let sub_filter = self.sub_filter.clone();
        // 引用过滤 + 计数命中总数，仅克隆前 MAX_RENDERED_MESSAGES 条，避免全量拷贝
        let (records, subs, matched_total): (Vec<MqttRecord>, Vec<Subscription>, usize) = self
            .with_app(cx, |app| {
                let subs: Vec<Subscription> = app
                    .subscriptions
                    .iter()
                    .filter(|s| s.connection_id == conn)
                    .cloned()
                    .collect();
                let mut matched = 0usize;
                let mut records: Vec<MqttRecord> = Vec::new();
                if let Some(m) = app.messages.get(&conn) {
                    for r in m.iter().rev() {
                        // 方向 / 搜索 / 订阅过滤叠加，过滤后再截断保证最新消息优先
                        let dir_ok = match dir {
                            DirFilter::All => true,
                            DirFilter::Received => r.direction == Direction::Received,
                            DirFilter::Published => r.direction == Direction::Published,
                        };
                        let query_ok = query.is_empty()
                            || contains_ignore_case(&r.topic, &query)
                            || contains_ignore_case(&r.payload, &query);
                        let sub_ok = sub_filter
                            .as_deref()
                            .map_or(true, |f| topic_matches(f, &r.topic));
                        if dir_ok && query_ok && sub_ok {
                            matched += 1;
                            if records.len() < MAX_RENDERED_MESSAGES {
                                records.push(r.clone());
                            }
                        }
                    }
                }
                (records, subs, matched)
            })
            .unwrap_or_default();

        let mut body = v_flex().flex_1().overflow_y_scrollbar();
        if records.is_empty() {
            let filtering = !query.is_empty()
                || sub_filter.is_some()
                || dir != DirFilter::All;
            body = body.child(
                v_flex()
                    .h_full()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .p_4()
                    .child(
                        div()
                            .size(px(56.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(cx.theme().muted)
                            .child(
                                gpui_kit::component::Icon::new(IconName::MailOpen)
                                    .size_6()
                                    .text_color(cx.theme().muted_foreground),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(if filtering {
                                "没有匹配的消息"
                            } else {
                                "暂无消息，订阅主题后消息会显示在这里"
                            }),
                    ),
            );
        } else {
            // 最新消息在最上方（与 MQTTX 一致，避免长列表需要手动滚底）
            for r in &records {
                let color = pick_subscription_color(&subs, &r.topic);
                body = body.child(self.render_message_row(r, color, cx));
            }
        }

        // 方向过滤 seg
        let dir_group = ButtonGroup::new(SharedString::from(format!(
            "msg-dir-{}",
            self.conn_id
        )))
        .compact()
        .children([DirFilter::All, DirFilter::Received, DirFilter::Published].into_iter().map(
            |d| {
                Button::new(SharedString::from(format!(
                    "msg-dir-btn-{}-{}",
                    self.conn_id,
                    d.index()
                )))
                .label(d.label())
                .small()
                .selected(d == dir)
            },
        ))
        .on_click(cx.listener(|this, ixs: &Vec<usize>, _, cx| {
            if let Some(&i) = ixs.first() {
                this.msg_dir = DirFilter::from_index(i);
                cx.notify();
            }
        }));

        // 订阅过滤 chip（点击订阅项激活）
        let filter_chip = self.sub_filter.clone().map(|topic| {
            h_flex()
                .gap_1()
                .items_center()
                .px_1p5()
                .py_0p5()
                .rounded_md()
                .bg(cx.theme().secondary)
                .max_w(px(180.))
                .child(
                    gpui_kit::component::Icon::new(IconName::ListFilter)
                        .xsmall()
                        .text_color(cx.theme().muted_foreground),
                )
                .child(
                    div()
                        .text_xs()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(topic),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "msg-filter-clear-{}",
                        self.conn_id
                    )))
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip("清除订阅过滤")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sub_filter = None;
                        cx.notify();
                    })),
                )
        });

        v_flex()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .h_11()
                    .px_3()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(dir_group)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(Input::new(&self.filter).small().prefix(
                                gpui_kit::component::Icon::new(IconName::Search).small(),
                            )),
                    )
                    .children(filter_chip)
                    .child(
                        div()
                            .text_xs()
                            .px_1p5()
                            .text_color(cx.theme().muted_foreground)
                            // 计数显示过滤命中总数，不受渲染截断影响
                            .child(format!("{} 条", matched_total)),
                    )
                    .when(matched_total > records.len(), |h| {
                        h.child(
                            div()
                                .text_xs()
                                .px_1p5()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("仅显示最新 {} 条", MAX_RENDERED_MESSAGES)),
                        )
                    })
                    .child({
                        let weak = cx.weak_entity();
                        let clear_id = self.conn_id.clone();
                        let empty = records.is_empty();
                        Button::new(SharedString::from(format!("msg-clear-{}", self.conn_id)))
                            .icon(IconName::Eraser)
                            .label("清空")
                            .ghost()
                            .small()
                            .disabled(empty)
                            .tooltip("清空当前连接的消息")
                            .on_click(move |_, window, cx| {
                                // 清空不可撤销，先弹二次确认
                                let weak = weak.clone();
                                let clear_id = clear_id.clone();
                                window.open_alert_dialog(cx, move |alert, _, _| {
                                    let weak = weak.clone();
                                    let clear_id = clear_id.clone();
                                    alert
                                        .title("清空消息")
                                        .description("确定清空当前连接的全部消息吗？此操作不可撤销。")
                                        .button_props(
                                            DialogButtonProps::default()
                                                .show_cancel(true)
                                                .cancel_text("取消")
                                                .ok_text("清空")
                                                .ok_variant(ButtonVariant::Danger),
                                        )
                                        .on_ok(move |_, _, cx| {
                                            weak.update(cx, |view, cx| {
                                                view.expanded = None;
                                                view.detail_owner = None;
                                                view.detail_format = DetailFormat::Auto;
                                                if let Some(app) = view.app.upgrade() {
                                                    app.update(cx, |app, cx| {
                                                        app.messages.remove(&clear_id);
                                                        cx.notify();
                                                    });
                                                }
                                                cx.notify();
                                            })
                                            .ok();
                                            true
                                        })
                                });
                            })
                    }),
            )
            .child(body)
    }

    fn render_logs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let conn = self.conn_id.clone();
        let show_millis = self
            .with_app(cx, |app| app.settings.show_millis)
            .unwrap_or(true);
        let logs: Vec<_> = self
            .with_app(cx, |app| {
                app.logs
                    .iter()
                    .rev()
                    .filter(|l| l.connection_id == conn)
                    // 与 app.rs 的 MAX_LOGS=3000 对齐，避免渲染截断早于存储上限
                    .take(MAX_RENDERED_LOGS)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        let mut body = v_flex().flex_1().overflow_y_scrollbar().p_2().gap_0p5();
        if logs.is_empty() {
            body = body.child(
                v_flex()
                    .p_4()
                    .gap_2()
                    .items_center()
                    .child(
                        gpui_kit::component::Icon::new(IconName::ScrollText)
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("暂无日志"),
                    ),
            );
        }
        let log_hover = cx.theme().muted;
        for l in logs {
            let color = match l.level {
                LogLevel::Error => cx.theme().danger,
                LogLevel::Warn => cx.theme().warning,
                LogLevel::Debug => cx.theme().muted_foreground,
                LogLevel::Info => cx.theme().foreground,
            };
            body = body.child(
                h_flex()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .hover(move |this| this.bg(log_hover))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(format_time(l.timestamp, show_millis)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .w_10()
                            .text_color(color)
                            .child(l.level.label()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .flex_1()
                            .min_w(px(0.))
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(format!("[{}] {}", l.event, l.message)),
                    ),
            );
        }
        v_flex()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .h_11()
                    .px_3()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .font_family(cx.theme().mono_font_family.clone())
                            .child("连接日志"),
                    ),
            )
            .child(body)
    }
    fn render_publish_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let pub_topic = self.pub_topic.clone();
        let payload = self.payload.clone();
        let pub_qos = self.pub_qos.clone();
        let payload_format = self.payload_format.clone();
        let content_type = self.content_type.clone();
        let msg_expiry = self.msg_expiry.clone();
        let response_topic = self.response_topic.clone();
        let correlation_data = self.correlation_data.clone();
        let user_props = self.user_props.clone();
        let retain = self.retain;
        let show_props = self.show_props;
        let show_vars = self.show_vars;
        let is_v5 = self.is_v5(cx);

        // 发布预设
        let presets = self.with_app(cx, |app| app.presets.clone()).unwrap_or_default();
        let connected = self.engine.is_connected(&self.conn_id);
        // 当前提取到的占位符数量（仅用于提示，行构建在 sync_var_rows 完成）
        let var_count = extract_placeholder_keys(
            &self.pub_topic.read(cx).value(),
            &self.payload.read(cx).value(),
        )
        .len();

        let mut card = v_flex()
            .w_full()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .px_3()
            .py_2()
            .gap_2();

        card = card.child(
            h_flex()
                .gap_2()
                .items_center()
                // Ctrl+Enter 快捷发送（焦点在主题输入上时）
                .child(
                    div()
                        .id(SharedString::from(format!("pub-topic-wrap-{}", self.conn_id)))
                        .flex_1()
                        .min_w(px(0.))
                        .on_key_down(cx.listener(
                            |this, ev: &gpui_kit::KeyDownEvent, window, cx| {
                                if ev.keystroke.modifiers.control && ev.keystroke.key == "enter" {
                                    cx.stop_propagation();
                                    this.do_publish(window, cx);
                                }
                            },
                        ))
                        .child(Input::new(&pub_topic).small()),
                )
                .child(div().w(px(92.)).child(Select::new(&pub_qos).small()))
                .child(div().w(px(124.)).child(Select::new(&payload_format).small()))
                .child(self.render_preset_menu(presets, cx))
                .child({
                    // 变量注入开关：展开/收起内联绑定面板
                    let count = var_count;
                    Button::new("vars-toggle")
                        .icon(IconName::Variable)
                        .ghost()
                        .small()
                        .when(show_vars, |b| b.selected(true))
                        .tooltip(if count > 0 {
                            format!("变量注入（{count} 个占位符）")
                        } else {
                            "变量注入".to_string()
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.show_vars {
                                this.show_vars = false;
                                cx.notify();
                            } else {
                                this.sync_var_rows(window, cx);
                            }
                        }))
                })
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(Switch::new("retain").checked(retain).on_change(
                            cx.listener(|this, v, _, cx| {
                                this.retain = *v;
                                cx.notify();
                            }),
                        ))
                        .child(div().text_xs().child("Retain")),
                )
                .when(is_v5, |h| {
                    h.child(
                        Button::new("toggle-props")
                            .icon(IconName::SlidersHorizontal)
                            .ghost()
                            .small()
                            .when(show_props, |b| b.selected(true))
                            .tooltip("MQTT 5 属性")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_props = !this.show_props;
                                cx.notify();
                            })),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!("publish-{}", self.conn_id)))
                        .icon(IconName::SendHorizontal)
                        .label("发送")
                        .primary()
                        .small()
                        // 未连接时禁用，Ctrl+Enter 路径由 do_publish 内的前置检查兜底
                        .disabled(!connected)
                        .when(!connected, |b| b.tooltip("未连接，无法发送"))
                        .on_click(cx.listener(|this, _, window, cx| this.do_publish(window, cx))),
                ),
        );

        card = card.child(
            div()
                .id(SharedString::from(format!("payload-wrap-{}", self.conn_id)))
                .h(px(110.))
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .p_1()
                // Ctrl+Enter 快捷发送（焦点在负载输入上时）
                .on_key_down(cx.listener(
                    |this, ev: &gpui_kit::KeyDownEvent, window, cx| {
                        if ev.keystroke.modifiers.control && ev.keystroke.key == "enter" {
                            cx.stop_propagation();
                            this.do_publish(window, cx);
                        }
                    },
                ))
                .child(Textarea::new(&payload)),
        );

        // 内联变量绑定面板：仅在展开且已提取到占位符时渲染
        if show_vars && !self.var_rows.is_empty() {
            card = card.child(self.render_vars_panel(cx));
        }

        if show_props && is_v5 {
            card = card.child(
                v_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        h_flex()
                            .gap_2()
                            .w_full()
                            .child(
                                div().w(px(220.)).child(field(
                                    "Content-Type",
                                    Input::new(&content_type).small(),
                                )),
                            )
                            .child(
                                div().w(px(120.)).child(field(
                                    "消息过期(秒)",
                                    Input::new(&msg_expiry).small(),
                                )),
                            )
                            .child(
                                div().flex_1().min_w(px(0.)).child(field(
                                    "Response Topic",
                                    Input::new(&response_topic).small(),
                                )),
                            )
                            .child(
                                div().flex_1().min_w(px(0.)).child(field(
                                    "Correlation Data",
                                    Input::new(&correlation_data).small(),
                                )),
                            ),
                    )
                    .child(user_props.clone()),
            );
        }
        card
    }

    /// 按当前 topic/payload 重新提取 `{{key}}`，增删变量输入行。
    /// 输入框创建需要 Window，因此只能在按钮回调里调用（无法在 render 中同步）。
    fn sync_var_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let topic = self.pub_topic.read(cx).value().to_string();
        let payload = self.payload.read(cx).value().to_string();
        let keys = extract_placeholder_keys(&topic, &payload);
        if keys.is_empty() {
            self.show_vars = false;
            window.push_notification(
                Notification::warning("主题/负载中没有 {{变量}} 引用"),
                cx,
            );
            cx.notify();
            return;
        }
        let vars = self.with_app(cx, |a| a.variables.clone()).unwrap_or_default();
        // 移除已不存在的 key（保留仍存在的行，避免丢未保存的输入）
        self.var_rows.retain(|(k, _, _)| keys.contains(k));
        for key in keys {
            if self.var_rows.iter().any(|(k, ..)| k == &key) {
                continue;
            }
            let initial = vars
                .iter()
                .find(|v| v.key == key)
                .map(|v| v.value.clone())
                .unwrap_or_default();
            let input = cx.new(|cx| {
                let mut s = InputState::new(window, cx).placeholder("变量值");
                // set_value 不会触发 InputEvent::Change，不会误写回
                if !initial.is_empty() {
                    s.set_value(initial.as_str(), window, cx);
                }
                s
            });
            // 编辑即写回全局变量（含持久化），预览与发布随之生效
            let key_for_sub = key.clone();
            let sub = cx.subscribe(&input, move |this, entity, ev: &InputEvent, cx| {
                if !matches!(ev, InputEvent::Change) {
                    return;
                }
                let val = entity.read(cx).value().to_string();
                let key = key_for_sub.clone();
                let Some(app) = this.app.upgrade() else {
                    return;
                };
                app.update(cx, |a, cx| {
                    match a.variables.iter_mut().find(|v| v.key == key) {
                        Some(v) => {
                            if v.value == val {
                                return;
                            }
                            v.value = val;
                        }
                        None => a.variables.push(GlobalVariable { key, value: val }),
                    }
                    let snapshot = a.variables.clone();
                    a.save_variables(snapshot);
                    cx.notify();
                });
            });
            self.var_rows.push((key, input, sub));
        }
        self.show_vars = true;
        cx.notify();
    }

    /// 变量绑定面板：逐 key 编辑 + 实时渲染预览。
    fn render_vars_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let vars = self.with_app(cx, |a| a.variables.clone()).unwrap_or_default();
        let topic_raw = self.pub_topic.read(cx).value().to_string();
        let payload_raw = self.payload.read(cx).value().to_string();
        let topic_rendered = render_template(&topic_raw, &vars);
        let payload_rendered = render_template(&payload_raw, &vars);
        let payload_preview: String = payload_rendered.chars().take(80).collect();
        let muted = cx.theme().muted_foreground;

        let mut rows = v_flex().gap_1();
        for (key, input, _) in &self.var_rows {
            rows = rows.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .w(px(160.))
                            .min_w(px(0.))
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{{{{{key}}}}}")),
                    )
                    .child(div().flex_1().min_w(px(0.)).child(Input::new(input).small())),
            );
        }

        v_flex()
            .gap_2()
            .w_full()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .child(format!("变量（{}）", self.var_rows.len())),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("vars-refresh")
                            .icon(IconName::RefreshCw)
                            .ghost()
                            .xsmall()
                            .tooltip("重新提取占位符")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.sync_var_rows(window, cx);
                            })),
                    )
                    .child(
                        Button::new("vars-collapse")
                            .icon(IconName::ChevronUp)
                            .ghost()
                            .xsmall()
                            .tooltip("收起")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_vars = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(rows)
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("内置：{{$ts}}（秒） {{$ts_ms}}（毫秒） {{$uuid}} · 发布时注入"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("预览：{topic_rendered} | {payload_preview}")),
            )
    }

    /// 发布预设下拉：应用已有预设、保存当前、删除预设。
    fn render_preset_menu(        &self,
        presets: Vec<crate::model::PublishPreset>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let weak_view = cx.weak_entity();
        let preset_name = self.preset_name.clone();
        let app_weak = self.app.clone();
        let has = !presets.is_empty();
        Button::new(SharedString::from(format!("preset-menu-{}", self.conn_id)))
            .icon(IconName::Bookmark)
            .ghost()
            .small()
            .tooltip("消息预设")
            .dropdown_menu(move |mut menu, window, cx| {
                // 应用预设
                for p in &presets {
                    let weak = weak_view.clone();
                    let params = p.params.clone();
                    menu = menu.item(
                        PopupMenuItem::new(SharedString::from(p.name.clone())).on_click(
                            move |_ev, window, cx| {
                                if let Some(view) = weak.upgrade() {
                                    view.update(cx, |view, cx| {
                                        view.apply_publish_preset(&params, window, cx);
                                    });
                                }
                            },
                        ),
                    );
                }
                if has {
                    menu = menu.separator();
                }
                // 保存当前
                let weak_save = weak_view.clone();
                let name_input = preset_name.clone();
                menu = menu.item(PopupMenuItem::new("保存当前为预设…").on_click(
                    move |_ev, window, cx| {
                        if let Some(view) = weak_save.upgrade() {
                            view.update(cx, |view, cx| {
                                view.open_save_preset_dialog(window, cx, name_input.clone());
                            });
                        }
                    },
                ));
                // 打开预设管理对话框（新建 / 编辑 / 删除）
                let manage_app = app_weak.clone();
                menu = menu.item(PopupMenuItem::new("管理预设…").on_click(
                    move |_ev, window, cx| {
                        if let Some(app) = manage_app.upgrade() {
                            crate::ui::presets_dialog::open(app, window, cx);
                        }
                    },
                ));
                // 删除子菜单
                if has {
                    let weak_del = weak_view.clone();
                    let del_presets = presets.clone();
                    menu = menu.submenu(
                        "删除预设",
                        window,
                        cx,
                        move |submenu, _window, _cx| {
                            let mut submenu = submenu;
                            for p in &del_presets {
                                let weak2 = weak_del.clone();
                                let id = p.id.clone();
                                let name = p.name.clone();
                                let label = format!("删除「{}」", p.name);
                                submenu = submenu.item(
                                    PopupMenuItem::new(SharedString::from(label)).on_click(
                                        move |_ev, window, cx| {
                                            // 预设删除不可撤销，先弹二次确认
                                            let weak = weak2.clone();
                                            let pid = id.clone();
                                            let pname = name.clone();
                                            window.open_alert_dialog(cx, move |alert, _, _| {
                                                let weak = weak.clone();
                                                let pid = pid.clone();
                                                alert
                                                    .title("删除预设")
                                                    .description(format!(
                                                        "确定删除预设「{pname}」吗？此操作不可撤销。"
                                                    ))
                                                    .button_props(
                                                        DialogButtonProps::default()
                                                            .show_cancel(true)
                                                            .cancel_text("取消")
                                                            .ok_text("删除")
                                                            .ok_variant(ButtonVariant::Danger),
                                                    )
                                                    .on_ok(move |_, _, cx| {
                                                        weak.update(cx, |view, cx| {
                                                            if let Some(app) = view.app.upgrade() {
                                                                app.update(cx, |app, cx| {
                                                                    app.delete_publish_preset(&pid);
                                                                    cx.notify();
                                                                });
                                                            }
                                                            cx.notify();
                                                        })
                                                        .ok();
                                                        true
                                                    })
                                            });
                                        },
                                    ),
                                );
                            }
                            submenu
                        },
                    );
                }
                menu
            })
    }

    /// 打开"保存为预设"命名对话框，保存当前发布参数。
    fn open_save_preset_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        name_input: Entity<InputState>,
    ) {
        // 预填默认名
        let topic = self.pub_topic.read(cx).value().to_string();
        let default_name = if topic.is_empty() {
            "新预设".to_string()
        } else {
            topic.chars().take(20).collect()
        };
        name_input.update(cx, |s, cx| s.set_value(default_name, window, cx));

        let weak = cx.weak_entity();
        let name_for_ok = name_input.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .w(px(420.))
                .title("保存消息预设")
                .child(v_flex().gap_3().child(
                    crate::ui::widgets::field("预设名称", Input::new(&name_for_ok)),
                ))
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .w_full()
                        .child(
                            Button::new("preset-save-cancel")
                                .label("取消")
                                .outline()
                                .on_click(move |_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("preset-save-ok")
                                .label("保存")
                                .primary()
                                .on_click({
                                    let name_input = name_for_ok.clone();
                                    let weak = weak.clone();
                                    move |_ev, window, cx| {
                                        let name = name_input
                                            .read(cx)
                                            .value()
                                            .trim()
                                            .to_string();
                                        if name.is_empty() {
                                            window.push_notification(
                                                Notification::warning("请填写预设名称"),
                                                cx,
                                            );
                                            return;
                                        }
                                        // 复用 do_publish 的过期秒数校验：输入了但解析不出即拒绝
                                        let params = weak
                                            .read_with(cx, |view, cx| {
                                                let expiry_raw = view
                                                    .msg_expiry
                                                    .read(cx)
                                                    .value()
                                                    .to_string();
                                                (
                                                    view.collect_publish_params(cx),
                                                    expiry_raw,
                                                )
                                            })
                                            .ok();
                                        if let (Some(view), Some((params, expiry_raw))) =
                                            (weak.upgrade(), params)
                                        {
                                            if !expiry_raw.trim().is_empty()
                                                && params.message_expiry_interval.is_none()
                                            {
                                                window.push_notification(
                                                    Notification::warning(
                                                        "消息过期须为非负整数（秒）",
                                                    ),
                                                    cx,
                                                );
                                                return;
                                            }
                                            if let Some(app) = view.read(cx).app.upgrade() {
                                                // 同名即覆盖，提示语区分以明确发生了什么
                                                let existed = app
                                                    .read(cx)
                                                    .presets
                                                    .iter()
                                                    .any(|p| p.name == name);
                                                app.update(cx, |app, cx| {
                                                    app.upsert_publish_preset(name.clone(), params);
                                                    cx.notify();
                                                });
                                                let msg = if existed {
                                                    format!("已覆盖预设「{name}」")
                                                } else {
                                                    "预设已保存".to_string()
                                                };
                                                window.push_notification(
                                                    Notification::success(msg),
                                                    cx,
                                                );
                                            }
                                        }
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
        });
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
                    .child(self.render_subscriptions(cx))
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
                                            .label("消息流")
                                            .selected(panel == Panel::Messages)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.panel = Panel::Messages;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Tab::new()
                                            .label("日志")
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
            .child(self.render_publish_bar(cx))
    }
}

// ─── 独立工具函数 ─────────────────────────────────────────────────────────────

/// 色相（度）→ gpui Hsla（h 分量为 0..=1 的整圆比例）。
/// 固定 L=0.52 时亮色相（黄绿）在浅色主题下过亮、暗色相（蓝靛）在深色主题下过暗，
/// 按色相段微调明度，保证两端主题下都可辨识。
fn hue_color(hue: f32) -> Hsla {
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
fn split_topic_list(raw: &str) -> Vec<String> {
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
fn extract_placeholder_keys(topic: &str, payload: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for text in [topic, payload] {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b'{' && bytes[i + 1] == b'{' {
                if let Some(rel_end) = text[i + 2..].find("}}") {
                    let key = text[i + 2..i + 2 + rel_end].trim();
                    if !key.is_empty() && !key.starts_with('$') && !out.iter().any(|k| k == key) {
                        out.push(key.to_string());
                    }
                    i = i + 2 + rel_end + 2;
                    continue;
                }
            }
            // 按字符推进，避免把多字节 UTF-8 拆开
            let ch = text[i..].chars().next().unwrap();
            i += ch.len_utf8();
        }
    }
    out
}

/// MQTT 主题过滤器匹配：`+` 匹配单层，`#` 匹配本层及以下（可匹配父级本身）。
fn topic_matches(filter: &str, topic: &str) -> bool {
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
fn pick_subscription_color(subs: &[Subscription], topic: &str) -> Option<f32> {
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
fn format_payload_detail(payload: &str, format: DetailFormat) -> String {
    match format {
        DetailFormat::Auto => serde_json::from_str::<serde_json::Value>(payload)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| payload.to_string()),
        DetailFormat::Text => payload.to_string(),
        DetailFormat::Hex => payload
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" "),
        DetailFormat::Base64 => {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.encode(payload.as_bytes())
        }
    }
}

/// 「复制详情」的可读文本：元数据 + v5 属性 + 用户属性 + 负载。
fn details_copy_text(record: &MqttRecord, show_millis: bool) -> String {
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
        for (k, v) in &record.user_properties {
            out.push_str(&format!("  {k} = {v}\n"));
        }
    }
    out.push_str(&format!("Payload:\n{}", record.payload));
    out
}

/// 折叠态只显示单行预览：换行/制表符压平成空格后截 200 字符。
/// 含替换字符（U+FFFD）说明负载不是合法文本，提示切 Hex 查看。
fn preview_payload(payload: &str) -> String {
    let flat: String = payload
        .chars()
        .map(|c| match c {
            '\n' | '\r' | '\t' => ' ',
            _ => c,
        })
        .collect();
    let mut out: String = flat.chars().take(200).collect();
    if flat.chars().count() > 200 {
        out.push('…');
    }
    if payload.contains('\u{FFFD}') {
        out.push_str("（非文本，详情可切 Hex）");
    }
    out
}

/// 大小写不敏感子串匹配。`needle` 已由调用方 to_lowercase 一次；
/// 先做零拷贝的字面匹配，未命中且原文含大写时才回退整串小写，
/// 避免渲染热路径对全部记录做 to_lowercase 拷贝。
fn contains_ignore_case(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if hay.contains(needle) {
        return true;
    }
    // 原文没有任何大写字符时，小写化不会改变匹配结果，直接判否
    if !hay.chars().any(|c| c.is_uppercase()) {
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
