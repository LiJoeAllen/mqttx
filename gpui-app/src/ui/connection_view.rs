//! 单个连接的工作区：订阅列表 + 消息流 + 发布面板 + 日志。

use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    h_flex, notification::Notification, v_flex, ActiveTheme as _, Selectable as _, Sizable as _,
    StyledExt as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use crate::ui::IconName;
use gpui_kit::{
    div, px, App, AppContext as _, Context, Entity, InteractiveElement as _, StatefulInteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window,
};
use gpui_kit::component::IndexPath;

use crate::model::{
    render_template, ConnectionConfig, ConnectionStatus, Direction, LogLevel, MqttRecord,
    PayloadFormat, PublishParams,
};
use crate::mqtt::MqttEngine;
use crate::ui::app::MqttXApp;
use crate::ui::widgets::{field, format_time, make_select, KvEditor, OptionDelegate};

const QOS: [&str; 3] = ["QoS 0", "QoS 1", "QoS 2"];
const PAYLOAD_FORMATS: [&str; 4] = ["Plaintext", "JSON", "Base64", "Hex"];
const MAX_RENDERED_MESSAGES: usize = 300;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Messages,
    Logs,
}

pub struct ConnectionView {
    conn_id: String,
    app: gpui_kit::WeakEntity<MqttXApp>,
    engine: Arc<MqttEngine>,

    // 订阅
    sub_topic: Entity<InputState>,
    sub_qos: Entity<SelectState<OptionDelegate>>,

    // 发布
    pub_topic: Entity<InputState>,
    payload: Entity<TextareaState>,
    pub_qos: Entity<SelectState<OptionDelegate>>,
    payload_format: Entity<SelectState<OptionDelegate>>,
    retain: bool,
    content_type: Entity<InputState>,
    user_props: Entity<KvEditor>,
    show_props: bool,

    // 过滤与面板
    filter: Entity<InputState>,
    panel: Panel,
    expanded: Option<u64>,

    // 预设
    preset_name: Entity<InputState>,
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
            InputState::new(window, cx).placeholder("订阅主题，支持通配符 # +")
        });
        let sub_qos = make_select(&QOS, 0, window, cx);
        let pub_topic = cx.new(|cx| InputState::new(window, cx).placeholder("发布主题"));
        let payload = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("输入消息负载，支持 {{变量名}} 与 {{$ts}} {{$uuid}}")
        });
        let pub_qos = make_select(&QOS, 0, window, cx);
        let payload_format = make_select(&PAYLOAD_FORMATS, 0, window, cx);
        let content_type = cx.new(|cx| InputState::new(window, cx).placeholder("Content-Type（可选）"));
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
            pub_topic,
            payload,
            pub_qos,
            payload_format,
            retain: false,
            content_type,
            user_props,
            show_props: false,
            filter,
            panel: Panel::Messages,
            expanded: None,
            preset_name,
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

    fn do_subscribe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let topic = self.sub_topic.read(cx).value().to_string();
        if topic.trim().is_empty() {
            window.push_notification(Notification::warning("订阅主题不能为空"), cx);
            return;
        }
        let qos = self.sub_qos.read(cx).selected_value().copied().unwrap_or(0) as u8;
        let sub = crate::model::Subscription::new(self.conn_id.clone(), topic, qos);
        self.engine.subscribe(self.conn_id.clone(), sub);
        self.sub_topic.update(cx, |s, cx| s.set_value("", window, cx));
    }

    fn do_publish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let topic = self.pub_topic.read(cx).value().to_string();
        if topic.trim().is_empty() {
            window.push_notification(Notification::warning("发布主题不能为空"), cx);
            return;
        }
        let raw_payload = self.payload.read(cx).value().to_string();

        let vars = self.with_app(cx, |app| app.variables.clone()).unwrap_or_default();
        let topic = render_template(&topic, &vars);
        let rendered = render_template(&raw_payload, &vars);

        let format = PayloadFormat::ALL[
            self.payload_format.read(cx).selected_value().copied().unwrap_or(0)
        ];
        if let PayloadFormat::Json = format
            && let Err(e) = serde_json::from_str::<serde_json::Value>(&rendered) {
                window.push_notification(
                    Notification::warning(format!("JSON 格式无效: {e}")),
                    cx,
                );
                return;
            }
        let encoded = match format.encode(&rendered) {
            Ok(bytes) => bytes,
            Err(e) => {
                window.push_notification(Notification::warning(e), cx);
                return;
            }
        };

        let is_v5 = self.config(cx).map(|c| c.protocol.is_v5()).unwrap_or(true);
        let mut params = PublishParams {
            topic,
            payload: rendered,
            payload_format: format,
            qos: self.pub_qos.read(cx).selected_value().copied().unwrap_or(0) as u8,
            retain: self.retain,
            raw_bytes: Some(encoded),
            ..Default::default()
        };
        if is_v5 {
            let ct = self.content_type.read(cx).value().to_string();
            params.content_type = (!ct.is_empty()).then_some(ct);
            params.user_properties = self.user_props.read(cx).pairs(cx);
        }
        self.engine.publish(self.conn_id.clone(), params);
    }

    /// 读取当前发布面板上的参数（模板渲染前的原始值）。
    fn collect_publish_params(&self, cx: &App) -> PublishParams {
        let qos = self.pub_qos.read(cx).selected_value().copied().unwrap_or(0) as u8;
        let format = PayloadFormat::ALL
            [self.payload_format.read(cx).selected_value().copied().unwrap_or(0)];
        PublishParams {
            topic: self.pub_topic.read(cx).value().to_string(),
            payload: self.payload.read(cx).value().to_string(),
            payload_format: format,
            qos,
            retain: self.retain,
            content_type: {
                let ct = self.content_type.read(cx).value().to_string();
                (!ct.is_empty()).then_some(ct)
            },
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

    // ── 渲染 ──────────────────────────────────────────────────────────────

    fn render_top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.conn_id.clone();
        let (name, status, is_v5) = self
            .with_app(cx, |app| {
                let cfg = app.connections.iter().find(|c| c.id == id);
                (
                    cfg.map(|c| c.name.clone()).unwrap_or_else(|| id.clone()),
                    app.statuses.get(&id).copied().unwrap_or(ConnectionStatus::Disconnected),
                    cfg.map(|c| c.protocol.is_v5()).unwrap_or(true),
                )
            })
            .unwrap_or((id.clone(), ConnectionStatus::Disconnected, true));
        let connected = matches!(status, ConnectionStatus::Connected);
        let _ = is_v5;

        let status_text = status.label();
        let status_tone = match status {
            ConnectionStatus::Connected => cx.theme().success,
            ConnectionStatus::Connecting => cx.theme().warning,
            ConnectionStatus::Error => cx.theme().danger,
            ConnectionStatus::Disconnected => cx.theme().muted_foreground,
        };

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
            .child(div().flex_1())
            .child(
                Button::new(SharedString::from(format!("conn-toggle-{}", self.conn_id)))
                    .icon(if connected {
                        IconName::Square
                    } else {
                        IconName::PlugZap
                    })
                    .label(if connected { "断开" } else { "连接" })
                    .when(connected, |b| b.danger().ghost())
                    .when(!connected, |b| b.primary().ghost())
                    .small()
                    .on_click(cx.listener(|this, _, _window, cx| {
                        let cfg = this.config(cx);
                        if let Some(cfg) = cfg {
                            if this.engine.is_connected(&cfg.id) {
                                this.engine.close(&cfg.id, true);
                            } else {
                                this.engine.connect(cfg);
                            }
                        }
                    })),
            )
    }

    fn render_subscribe_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let sub_topic = self.sub_topic.clone();
        let sub_qos = self.sub_qos.clone();
        h_flex()
            .gap_2()
            .px_3()
            .h_12()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(div().flex_1().min_w(px(0.)).child(Input::new(&sub_topic).small()))
            .child(div().w(px(96.)).child(Select::new(&sub_qos).small()))
            .child(
                Button::new(SharedString::from(format!("subscribe-{}", self.conn_id)))
                    .icon(IconName::Plus)
                    .label("订阅")
                    .primary()
                    .small()
                    .on_click(cx.listener(|this, _, window, cx| this.do_subscribe(window, cx))),
            )
    }

    fn render_subscriptions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hover_bg = cx.theme().muted;
        let conn = self.conn_id.clone();
        let subs: Vec<_> = self
            .with_app(cx, |app| {
                app.subscriptions
                    .iter()
                    .filter(|s| s.connection_id == conn)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

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
            let topic = sub.topic.clone();
            let topic_for_click = topic.clone();
            let cid = self.conn_id.clone();
            list = list.child(
                h_flex()
                    .id(SharedString::from(format!("sub-{}-{}", self.conn_id, sub.id)))
                    .gap_1p5()
                    .items_center()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .hover(move |this| this.bg(hover_bg))
                    .child(
                        gpui_kit::component::Icon::new(IconName::Hash)
                            .xsmall()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_sm()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(topic),
                    )
                    .child(
                        div()
                            .text_xs()
                            .px_1()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Q{}", sub.qos)),
                    )
                    .child(
                        Button::new(SharedString::from(format!("sub-del-{}", sub.id)))
                            .icon(IconName::Close)
                            .ghost()
                            .xsmall()
                            .tooltip("取消订阅")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(app) = this.app.upgrade() {
                                    app.update(cx, |app, cx| {
                                        app.remove_subscription(&cid, &topic_for_click);
                                        cx.notify();
                                    });
                                }
                            })),
                    ),
            );
        }

        v_flex()
            .w(px(220.))
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

    fn render_message_row(&self, record: &MqttRecord, cx: &mut Context<Self>) -> impl IntoElement {
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
        let direction_label = if received { "接收" } else { "发送" };

        let mut row = v_flex()
            .id(SharedString::from(format!(
                "msg-{}-{}",
                record.connection_id, record.seq
            )))
            .w_full()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(border)
            .hover(move |this| this.bg(hover_bg))
            .cursor_pointer()
            .child(
                h_flex()
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
                            .child(format_time(record.timestamp, true)),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .pt_0p5()
                    .font_family(mono.clone())
                    .child(preview_payload(&record.payload, expanded)),
            );

        if expanded {
            let mut details = v_flex().gap_1p5().pt_1();
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&record.payload) {
                details = details.child(
                    div()
                        .font_family(mono.clone())
                        .text_xs()
                        .p_2()
                        .rounded_md()
                        .bg(cx.theme().background)
                        .border_1()
                        .border_color(border)
                        .child(serde_json::to_string_pretty(&v).unwrap_or_default()),
                );
            }
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
            row = row.child(
                div()
                    .mt_1()
                    .p_2()
                    .rounded_lg()
                    .bg(cx.theme().muted)
                    .child(details),
            );
        }
        row.on_click(cx.listener(move |this, _, _, cx| {
            this.expanded = if this.expanded == Some(seq) {
                None
            } else {
                Some(seq)
            };
            cx.notify();
        }))
    }

    fn render_messages(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.filter.read(cx).value().to_lowercase();
        let conn = self.conn_id.clone();
        let records: Vec<MqttRecord> = self
            .with_app(cx, |app| {
                app.messages
                    .get(&conn)
                    .map(|m| {
                        m.iter()
                            .rev()
                            .take(MAX_RENDERED_MESSAGES)
                            .filter(|r| {
                                query.is_empty()
                                    || r.topic.to_lowercase().contains(&query)
                                    || r.payload.to_lowercase().contains(&query)
                            })
                            .cloned()
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        let mut body = v_flex().flex_1().overflow_y_scrollbar();
        if records.is_empty() {
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
                            .child(if query.is_empty() {
                                "暂无消息，订阅主题后消息会显示在这里"
                            } else {
                                "没有匹配的消息"
                            }),
                    ),
            );
        } else {
            // 最新消息在最上方（与 MQTTX 一致，避免长列表需要手动滚底）
            for r in &records {
                body = body.child(self.render_message_row(r, cx));
            }
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
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(Input::new(&self.filter).small().prefix(
                                gpui_kit::component::Icon::new(IconName::Search).small(),
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .px_1p5()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} 条", records.len())),
                    ),
            )
            .child(body)
    }

    fn render_logs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let conn = self.conn_id.clone();
        let logs: Vec<_> = self
            .with_app(cx, |app| {
                app.logs
                    .iter()
                    .rev()
                    .filter(|l| l.connection_id == conn)
                    .take(2000)
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
                            .child(format_time(l.timestamp, true)),
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
        let user_props = self.user_props.clone();
        let retain = self.retain;
        let show_props = self.show_props;
        let is_v5 = self
            .config(cx)
            .map(|c| c.protocol.is_v5())
            .unwrap_or(true);

        // 发布预设
        let presets = self.with_app(cx, |app| app.presets.clone()).unwrap_or_default();

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
                .child(div().flex_1().min_w(px(0.)).child(Input::new(&pub_topic).small()))
                .child(div().w(px(92.)).child(Select::new(&pub_qos).small()))
                .child(div().w(px(124.)).child(Select::new(&payload_format).small()))
                .child(self.render_preset_menu(presets, cx))
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
                        .on_click(cx.listener(|this, _, window, cx| this.do_publish(window, cx))),
                ),
        );

        card = card.child(
            div()
                .h(px(110.))
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .p_1()
                .child(Textarea::new(&payload)),
        );

        if show_props && is_v5 {
            card = card.child(
                h_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .w(px(240.))
                            .child(field("Content-Type", Input::new(&content_type).small())),
                    )
                    .child(div().flex_1().min_w(px(0.)).child(user_props.clone())),
            );
        }
        card
    }

    /// 发布预设下拉：应用已有预设、保存当前、删除预设。
    fn render_preset_menu(
        &self,
        presets: Vec<crate::model::PublishPreset>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let weak_view = cx.weak_entity();
        let preset_name = self.preset_name.clone();
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
                                let label = format!("删除「{}」", p.name);
                                submenu = submenu.item(
                                    PopupMenuItem::new(SharedString::from(label)).on_click(
                                        move |_ev, _w, cx| {
                                            if let Some(view) = weak2.upgrade() {
                                                view.update(cx, |view, cx| {
                                                    if let Some(app) = view.app.upgrade() {
                                                        app.update(cx, |app, cx| {
                                                            app.delete_publish_preset(&id);
                                                            cx.notify();
                                                        });
                                                    }
                                                });
                                            }
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
                                        let params = weak
                                            .read_with(cx, |view, cx| {
                                                view.collect_publish_params(cx)
                                            })
                                            .ok();
                                        if let (Some(view), Some(params)) =
                                            (weak.upgrade(), params)
                                            && let Some(app) = view.read(cx).app.upgrade() {
                                                app.update(cx, |app, cx| {
                                                    app.upsert_publish_preset(name, params);
                                                    cx.notify();
                                                });
                                                window.push_notification(
                                                    Notification::success("预设已保存"),
                                                    cx,
                                                );
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

fn preview_payload(payload: &str, expanded: bool) -> String {
    if expanded {
        payload.to_string()
    } else {
        let one_line: String = payload.chars().take(200).collect();
        if payload.len() > 200 {
            format!("{one_line}…")
        } else {
            one_line
        }
    }
}
