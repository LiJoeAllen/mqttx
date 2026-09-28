//! 新建/编辑连接的表单对话框内容。

use std::sync::Arc;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    h_flex, notification::Notification, v_flex, ActiveTheme as _, Icon, Sizable as _,
    StyledExt as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use crate::ui::IconName;
use gpui_kit::{
    div, percentage, px, App, AppContext as _, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window,
};

use crate::model::{ConnectionConfig, LastWill, ProtocolVersion, TransportKind};
use crate::mqtt::MqttEngine;
use crate::ui::widgets::{field, make_select, OptionDelegate};

const PROTOCOLS: [&str; 2] = ["MQTT 5.0", "MQTT 3.1.1"];
const TRANSPORTS: [&str; 4] = ["TCP", "TLS", "WebSocket", "WSS"];
const QOS: [&str; 3] = ["QoS 0", "QoS 1", "QoS 2"];

pub struct ConnectionForm {
    engine: Arc<MqttEngine>,
    editing_id: Option<String>,
    created_at: i64,

    name: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    path: Entity<InputState>,
    client_id: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    keep_alive: Entity<InputState>,
    session_expiry: Entity<InputState>,
    receive_max: Entity<InputState>,
    max_packet: Entity<InputState>,
    topic_alias: Entity<InputState>,

    will_topic: Entity<InputState>,
    will_payload: Entity<InputState>,

    protocol: Entity<SelectState<OptionDelegate>>,
    transport: Entity<SelectState<OptionDelegate>>,
    will_qos: Entity<SelectState<OptionDelegate>>,

    clean_start: bool,
    auto_resubscribe: bool,
    auto_reconnect: bool,
    will_enabled: bool,
    will_retain: bool,

    // 按需显示：高级分组默认折叠
    show_auth: bool,
    show_mqtt5: bool,
    show_will: bool,

    testing: bool,
    _subs: Vec<Subscription>,
}

impl ConnectionForm {
    /// 对话框 footer 的测试按钮需要读取测试进行中的状态。
    pub fn is_testing(&self) -> bool {
        self.testing
    }
}

fn input(
    value: &str,
    placeholder: &str,
    window: &mut Window,
    cx: &mut Context<ConnectionForm>,
) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value)
    })
}

fn optional_input(
    value: Option<u32>,
    placeholder: &str,
    window: &mut Window,
    cx: &mut Context<ConnectionForm>,
) -> Entity<InputState> {
    cx.new(|cx| {
        let s = InputState::new(window, cx).placeholder(placeholder);
        match value {
            Some(v) => s.default_value(v.to_string()),
            None => s,
        }
    })
}

impl ConnectionForm {
    pub fn new(
        engine: Arc<MqttEngine>,
        edit: Option<ConnectionConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let c = edit.clone().unwrap_or_default();
        let editing_id = edit.as_ref().map(|e| e.id.clone());
        let created_at = edit.as_ref().map(|e| e.created_at).unwrap_or_else(|| {
            chrono::Local::now().timestamp()
        });

        let name = input(&c.name, "连接名称", window, cx);
        let host = input(&c.host, "broker.example.com", window, cx);
        let port = input(&c.port.to_string(), "端口", window, cx);
        let path = input(&c.path, "/mqtt", window, cx);
        let client_id = input(&c.client_id, "Client ID", window, cx);
        let username = input(&c.username, "用户名（可选）", window, cx);
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("密码（可选）")
                .default_value(&c.password)
                .masked(true)
        });
        let keep_alive = input(&c.keep_alive.to_string(), "60", window, cx);
        let session_expiry = input(
            &if c.session_expiry_interval > 0 {
                c.session_expiry_interval.to_string()
            } else {
                String::new()
            },
            "秒（0=会话随连接结束）",
            window,
            cx,
        );
        let receive_max = optional_input(c.receive_maximum.map(u32::from), "服务端接收上限", window, cx);
        let max_packet = optional_input(c.maximum_packet_size, "最大报文长度", window, cx);
        let topic_alias = optional_input(c.topic_alias_maximum.map(u32::from), "主题别名上限", window, cx);

        let (will_topic, will_payload, will_enabled, will_retain, will_qos_idx) =
            match &c.last_will {
                Some(w) => (
                    input(&w.topic, "遗嘱主题", window, cx),
                    input(&w.payload, "遗嘱消息", window, cx),
                    true,
                    w.retain,
                    w.qos as usize,
                ),
                None => (
                    input("", "遗嘱主题", window, cx),
                    input("", "遗嘱消息", window, cx),
                    false,
                    false,
                    0,
                ),
            };

        let protocol = make_select(
            &PROTOCOLS,
            if c.protocol.is_v5() { 0 } else { 1 },
            window,
            cx,
        );
        let transport_idx = TransportKind::ALL
            .iter()
            .position(|t| *t == c.transport)
            .unwrap_or(0);
        let transport = make_select(&TRANSPORTS, transport_idx, window, cx);
        let will_qos = make_select(&QOS, will_qos_idx, window, cx);

        // 切换传输方式时，把端口同步成该方式的默认端口，并刷新 WS 路径的按需显示。
        let port_for_sub = port.clone();
        let sub_transport = cx.subscribe_in(
            &transport,
            window,
            move |_this, state, event, window, cx| {
                if matches!(event, SelectEvent::Confirm(Some(_)))
                    && let Some(idx) = state.read(cx).selected_value() {
                        let kind = TransportKind::ALL[*idx];
                        port_for_sub.update(cx, |p, cx| {
                            p.set_value(kind.default_port().to_string(), window, cx);
                        });
                        cx.notify();
                    }
            },
        );
        // 切换协议时刷新 MQTT 5 属性分组的按需显示。
        let sub_protocol = cx.subscribe_in(
            &protocol,
            window,
            move |_this, _state, event, _window, cx| {
                if matches!(event, SelectEvent::Confirm(Some(_))) {
                    cx.notify();
                }
            },
        );

        Self {
            engine,
            editing_id,
            created_at,
            name,
            host,
            port,
            path,
            client_id,
            username,
            password,
            keep_alive,
            session_expiry,
            receive_max,
            max_packet,
            topic_alias,
            will_topic,
            will_payload,
            protocol,
            transport,
            will_qos,
            clean_start: c.clean_start,
            auto_resubscribe: c.auto_resubscribe,
            auto_reconnect: c.auto_reconnect,
            will_enabled,
            will_retain,
            show_auth: true,
            show_mqtt5: false,
            show_will: false,
            testing: false,
            _subs: vec![sub_transport, sub_protocol],
        }
    }

    fn val(&self, e: &Entity<InputState>, cx: &App) -> String {
        e.read(cx).value().to_string()
    }

    fn parse_u16(s: &str, label: &str) -> Result<u16, String> {
        s.trim()
            .parse::<u16>()
            .map_err(|_| format!("{label} 需要是 0~65535 的整数"))
    }

    fn parse_opt_u32(s: &str, label: &str) -> Result<Option<u32>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        s.parse::<u32>()
            .map(Some)
            .map_err(|_| format!("{label} 需要是非负整数"))
    }

    pub fn build(&self, cx: &App) -> Result<ConnectionConfig, String> {
        self.build_inner(cx, false)
    }

    /// 测试连接专用的宽松构建：名称与 Client ID 不阻塞测试
    ///（握手时会生成临时唯一 Client ID）。
    fn build_for_test(&self, cx: &App) -> Result<ConnectionConfig, String> {
        self.build_inner(cx, true)
    }

    fn build_inner(&self, cx: &App, for_test: bool) -> Result<ConnectionConfig, String> {
        let mut name = self.val(&self.name, cx);
        if name.trim().is_empty() {
            if !for_test {
                return Err("连接名称不能为空".into());
            }
            name = "测试连接".into();
        }
        let host = self.val(&self.host, cx);
        if host.trim().is_empty() {
            return Err("主机地址不能为空".into());
        }
        let port = Self::parse_u16(&self.val(&self.port, cx), "端口")?;
        let mut client_id = self.val(&self.client_id, cx);
        if client_id.trim().is_empty() {
            if !for_test {
                return Err("Client ID 不能为空".into());
            }
            client_id = format!("mqttx-test-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        }
        let keep_alive = Self::parse_u16(&self.val(&self.keep_alive, cx), "Keep Alive")?;
        let session_expiry = self
            .val(&self.session_expiry, cx)
            .trim()
            .parse::<u32>()
            .unwrap_or(0);
        let receive_maximum = Self::parse_opt_u32(&self.val(&self.receive_max, cx), "接收上限")?
            .map(|v| u16::try_from(v).map_err(|_| "接收上限不能超过 65535".to_string()))
            .transpose()?;
        let maximum_packet_size =
            Self::parse_opt_u32(&self.val(&self.max_packet, cx), "最大报文长度")?;
        let topic_alias_maximum = Self::parse_opt_u32(&self.val(&self.topic_alias, cx), "主题别名")?
            .map(|v| u16::try_from(v).map_err(|_| "主题别名上限不能超过 65535".to_string()))
            .transpose()?;

        let protocol = match self.protocol.read(cx).selected_value() {
            Some(1) => ProtocolVersion::V311,
            _ => ProtocolVersion::V5,
        };
        let transport = self
            .transport
            .read(cx)
            .selected_value()
            .and_then(|i| TransportKind::ALL.get(*i).copied())
            .unwrap_or_default();

        let last_will = if self.will_enabled {
            let topic = self.val(&self.will_topic, cx);
            if topic.trim().is_empty() {
                return Err("遗嘱主题不能为空".into());
            }
            Some(LastWill {
                topic,
                payload: self.val(&self.will_payload, cx),
                qos: self.will_qos.read(cx).selected_value().copied().unwrap_or(0) as u8,
                retain: self.will_retain,
                content_type: None,
                response_topic: None,
            })
        } else {
            None
        };

        let id = self.editing_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Ok(ConnectionConfig {
            id,
            name,
            host,
            port,
            path: self.val(&self.path, cx),
            protocol,
            transport,
            client_id,
            username: self.val(&self.username, cx),
            password: self.val(&self.password, cx),
            clean_start: self.clean_start,
            keep_alive,
            session_expiry_interval: session_expiry,
            receive_maximum,
            maximum_packet_size,
            topic_alias_maximum,
            auto_resubscribe: self.auto_resubscribe,
            auto_reconnect: self.auto_reconnect,
            last_will,
            created_at: self.created_at,
        })
    }

    pub fn run_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut cfg = match self.build_for_test(cx) {
            Ok(c) => c,
            Err(e) => {
                window.push_notification(Notification::error(e), cx);
                return;
            }
        };
        // 测试连接用临时 id，不影响已有连接
        cfg.id = uuid::Uuid::new_v4().to_string();
        let rx = self.engine.test_connection(cfg);
        self.testing = true;
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let result = rx.recv().await.unwrap_or(Err("测试任务丢失".into()));
            this.update_in(cx, |form, window, cx| {
                form.testing = false;
                match result {
                    Ok(()) => window.push_notification(
                        Notification::success("连接测试成功"),
                        cx,
                    ),
                    Err(e) => window.push_notification(Notification::error(format!(
                        "连接测试失败: {e}"
                    )), cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn switch_row(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .justify_between()
            .child(div().text_sm().child(label))
            .child(
                Switch::new(id)
                    .checked(checked)
                    .on_change(cx.listener(move |this, v, _, cx| {
                        let v = *v;
                        match id {
                            "clean_start" => this.clean_start = v,
                            "auto_resubscribe" => this.auto_resubscribe = v,
                            "auto_reconnect" => this.auto_reconnect = v,
                            "will_enabled" => this.will_enabled = v,
                            "will_retain" => this.will_retain = v,
                            _ => {}
                        }
                        cx.notify();
                    })),
            )
    }

    /// 可折叠分组：点击标题行切换展开/收起，收起时不渲染内容。
    #[allow(clippy::too_many_arguments)]
    fn collapsible_section(
        &self,
        id: &'static str,
        title: &'static str,
        hint: Option<SharedString>,
        hint_accent: bool,
        open: bool,
        toggle: fn(&mut Self),
        border: gpui_kit::Hsla,
        bg: gpui_kit::Hsla,
        body: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let title: SharedString = title.into();
        v_flex().gap_2().child(
            v_flex()
                .child(
                    h_flex()
                        .id(id)
                        .justify_between()
                        .rounded_md()
                        .px_1()
                        .py_1()
                        .cursor_pointer()
                        .hover(|s| s.bg(cx.theme().secondary_hover))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            toggle(this);
                            cx.notify();
                        }))
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .child(div().text_sm().font_semibold().child(title))
                                .children(hint.map(|h| {
                                    let color = if hint_accent {
                                        cx.theme().accent
                                    } else {
                                        cx.theme().muted_foreground
                                    };
                                    div().text_xs().text_color(color).child(h)
                                })),
                        )
                        .child(
                            Icon::new(IconName::ChevronDown)
                                .xsmall()
                                .text_color(cx.theme().muted_foreground)
                                .rotate(percentage(if open { 0.5 } else { 0. })),
                        ),
                )
                .when(open, |w| {
                    w.child(
                        div()
                            .rounded_lg()
                            .border_1()
                            .border_color(border)
                            .bg(bg)
                            .p_3()
                            .child(body),
                    )
                }),
        )
    }
}

impl Render for ConnectionForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let card_bg = cx.theme().secondary;
        // 协议/传输决定高级分组的按需显示
        let is_v5 = self.protocol.read(cx).selected_value() != Some(&1);
        let is_ws = matches!(
            self.transport.read(cx).selected_value(),
            Some(2) | Some(3)
        );
        v_flex()
            .gap_3()
            .max_h(px(560.))
            .overflow_y_scrollbar()
            .pr_1()
            // ── 基本信息（始终展开） ──
            .child(section("基本信息", border, card_bg,
                v_flex().gap_2().child(
                    h_flex().gap_2().w_full()
                        .child(div().flex_1().min_w(px(0.)).child(field("名称", Input::new(&self.name))))
                        .child(div().w(px(120.)).child(field("端口", Input::new(&self.port))))
                )
                .child(h_flex().gap_2().w_full()
                    .child(div().flex_1().min_w(px(0.)).child(field("主机", Input::new(&self.host))))
                    .child(div().flex_1().min_w(px(0.)).child(field("协议", Select::new(&self.protocol))))
                    .child(div().flex_1().min_w(px(0.)).child(field("传输", Select::new(&self.transport))))
                )
                .child(h_flex().gap_2().w_full()
                    .child(div().flex_1().min_w(px(0.)).child(field("Client ID", Input::new(&self.client_id))))
                    // WS 路径仅对 WebSocket/WSS 有意义，按需显示
                    .when(is_ws, |w| w.child(
                        div().flex_1().min_w(px(0.)).child(field("WS 路径", Input::new(&self.path)))
                    ))
                )
            ))
            // ── 认证与心跳 ──
            .child(self.collapsible_section(
                "sec-auth", "认证与心跳", None, false, self.show_auth,
                |f| f.show_auth = !f.show_auth, border, card_bg,
                v_flex().gap_2()
                    .child(h_flex().gap_2().w_full()
                        .child(div().flex_1().min_w(px(0.)).child(field("用户名", Input::new(&self.username))))
                        .child(div().flex_1().min_w(px(0.)).child(field("密码", Input::new(&self.password).mask_toggle())))
                        .child(div().w(px(110.)).child(field("Keep Alive (秒)", Input::new(&self.keep_alive))))
                    )
                    .child(self.switch_row("clean_start", "Clean Start / Clean Session", self.clean_start, cx))
                    .child(self.switch_row("auto_reconnect", "断线自动重连", self.auto_reconnect, cx))
                    .child(self.switch_row("auto_resubscribe", "连接后自动恢复订阅", self.auto_resubscribe, cx)),
                cx,
            ))
            // ── MQTT 5 高级属性（仅 v5 协议显示） ──
            .when(is_v5, |w| w.child(self.collapsible_section(
                "sec-mqtt5", "MQTT 5 属性", None, false, self.show_mqtt5,
                |f| f.show_mqtt5 = !f.show_mqtt5, border, card_bg,
                h_flex().gap_2().w_full()
                    .child(div().flex_1().min_w(px(0.)).child(field("会话过期间隔(秒)", Input::new(&self.session_expiry))))
                    .child(div().flex_1().min_w(px(0.)).child(field("接收最大值", Input::new(&self.receive_max))))
                    .child(div().flex_1().min_w(px(0.)).child(field("最大报文长度", Input::new(&self.max_packet))))
                    .child(div().flex_1().min_w(px(0.)).child(field("主题别名上限", Input::new(&self.topic_alias)))),
                cx,
            )))
            // ── 遗嘱 ──
            .child(self.collapsible_section(
                "sec-will", "遗嘱消息 (Last Will)",
                self.will_enabled.then(|| "已启用".into()), true, self.show_will,
                |f| f.show_will = !f.show_will, border, card_bg,
                v_flex().gap_2()
                    .child(self.switch_row("will_enabled", "启用遗嘱消息", self.will_enabled, cx))
                    .when(self.will_enabled, |w| w
                        .child(h_flex().gap_2()
                            .child(div().flex_1().min_w(px(0.)).child(field("遗嘱主题", Input::new(&self.will_topic))))
                            .child(div().w(px(130.)).child(field("QoS", Select::new(&self.will_qos))))
                        )
                        .child(field("遗嘱内容", Input::new(&self.will_payload)))
                        .child(self.switch_row("will_retain", "Retain", self.will_retain, cx))
                    ),
                cx,
            ))
    }
}

fn section(
    title: impl Into<gpui_kit::SharedString>,
    border: gpui_kit::Hsla,
    bg: gpui_kit::Hsla,
    body: impl IntoElement,
) -> impl IntoElement {
    let title: gpui_kit::SharedString = title.into();
    v_flex()
        .gap_2()
        .child(div().text_sm().font_semibold().child(title))
        .child(
            div()
                .rounded_lg()
                .border_1()
                .border_color(border)
                .bg(bg)
                .p_3()
                .child(body),
        )
}
