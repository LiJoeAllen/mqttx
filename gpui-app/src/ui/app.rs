//! 应用外壳：标题栏 + 连接侧边栏 + 多标签连接视图 + 引擎事件泵。

use std::collections::{HashMap, VecDeque};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    h_flex, notification::Notification, v_flex, ActiveTheme as _, Selectable as _, Sizable as _,
    StyledExt as _, Theme, ThemeMode, TitleBar, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, AppContext as _, Context, Entity, InteractiveElement as _, StatefulInteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window,
};

use crate::aliyun::AliyunPreset;
use crate::model::{
    AppSettings, ConnectionConfig, ConnectionStatus, Direction, GlobalVariable, LogEntry,
    LogLevel, MqttRecord, PublishParams, PublishPreset, Subscription, ThemeModePref,
};
use crate::mqtt::{EngineEvent, MqttEngine};
use crate::store::Storage;
use crate::ui::IconName;
use crate::ui::{
    connection_form::ConnectionForm,
    connection_view::ConnectionView,
    widgets::status_color,
};

const MAX_LOGS: usize = 3000;

pub struct MqttXApp {
    storage: Storage,
    pub engine: std::sync::Arc<MqttEngine>,
    _pump: gpui_kit::Task<()>,

    // ── 持久化数据 ──
    pub connections: Vec<ConnectionConfig>,
    pub subscriptions: Vec<Subscription>,
    pub presets: Vec<PublishPreset>,
    pub variables: Vec<GlobalVariable>,
    pub aliyun_presets: Vec<AliyunPreset>,
    pub settings: AppSettings,

    // ── 运行时状态 ──
    pub statuses: HashMap<String, ConnectionStatus>,
    pub errors: HashMap<String, String>,
    pub messages: HashMap<String, Vec<MqttRecord>>,
    pub logs: VecDeque<LogEntry>,
    pub seq: u64,

    // ── 标签与视图 ──
    open_tabs: Vec<String>,
    active_tab: Option<String>,
    views: HashMap<String, Entity<ConnectionView>>,

    search: Entity<InputState>,
}

impl MqttXApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let storage = Storage::new();
        let connections = storage.load_connections();
        let subscriptions = storage.load_subscriptions();
        let presets = storage.load_presets();
        let variables = storage.load_variables();
        let aliyun_presets = storage.load_aliyun();
        let settings = storage.load_settings();

        apply_theme(settings.theme, cx);

        let (tx, rx) = smol::channel::unbounded();
        let engine = MqttEngine::new(tx);

        let weak = cx.entity().downgrade();
        let pump = cx.spawn_in(window, async move |_this, cx: &mut gpui_kit::AsyncWindowContext| {
            while let Ok(event) = rx.recv().await {
                weak.update_in(cx, |app, window, cx| {
                    app.on_engine_event(event, window, cx);
                })
                .ok();
            }
        });

        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索连接…"));

        
        Self {
            storage,
            engine,
            _pump: pump,
            connections,
            subscriptions,
            presets,
            variables,
            aliyun_presets,
            settings,
            statuses: HashMap::new(),
            errors: HashMap::new(),
            messages: HashMap::new(),
            logs: VecDeque::with_capacity(MAX_LOGS),
            seq: 0,
            open_tabs: Vec::new(),
            active_tab: None,
            views: HashMap::new(),
            search,
        }
    }

    // ── 引擎事件 ──────────────────────────────────────────────────────────

    fn on_engine_event(&mut self, event: EngineEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            EngineEvent::Status {
                connection_id,
                status,
                error,
                ..
            } => {
                self.statuses.insert(connection_id.clone(), status);
                match error {
                    Some(e) => {
                        self.errors.insert(connection_id.clone(), e);
                    }
                    None => {
                        self.errors.remove(&connection_id);
                    }
                }
                if status == ConnectionStatus::Connected {
                    self.on_connected(&connection_id);
                }
                if let Some(view) = self.views.get(&connection_id) {
                    view.update(cx, |_, cx| cx.notify());
                }
                cx.notify();
            }
            EngineEvent::Message(mut record) => {
                let id = record.connection_id.clone();
                record.seq = self.next_seq();
                self.push_message(id.clone(), record);
                if let Some(view) = self.views.get(&id) {
                    view.update(cx, |_, cx| cx.notify());
                }
            }
            EngineEvent::Published {
                connection_id,
                topic,
                payload,
                qos,
                retain,
                content_type,
                user_properties,
            } => {
                let record = MqttRecord {
                    seq: self.next_seq(),
                    connection_id: connection_id.clone(),
                    direction: Direction::Published,
                    topic,
                    payload,
                    qos,
                    retain,
                    timestamp: chrono::Local::now().timestamp_millis(),
                    user_properties,
                    content_type,
                    response_topic: None,
                    correlation_data: None,
                    message_expiry_interval: None,
                    subscription_identifier: None,
                };
                self.push_message(connection_id.clone(), record);
                if let Some(view) = self.views.get(&connection_id) {
                    view.update(cx, |_, cx| cx.notify());
                }
            }
            EngineEvent::SubscribeResult {
                connection_id,
                topic,
                qos,
                ok,
                error,
            } => {
                if ok {
                    self.add_subscription(Subscription::new(connection_id.clone(), topic, qos));
                } else if let Some(e) = error {
                    window.push_notification(
                        Notification::error(format!("订阅失败: {e}")),
                        cx,
                    );
                }
                if let Some(view) = self.views.get(&connection_id) {
                    view.update(cx, |_, cx| cx.notify());
                }
            }
            EngineEvent::Log(entry) => {
                self.push_log(entry);
                if let Some(id) = self.active_tab.clone()
                    && let Some(view) = self.views.get(&id) {
                        view.update(cx, |_, cx| cx.notify());
                    }
            }
        }
    }

    fn on_connected(&mut self, connection_id: &str) {
        // 连接成功后自动恢复已保存订阅（auto resubscribe）。
        let Some(config) = self.connections.iter().find(|c| c.id == connection_id).cloned() else {
            return;
        };
        if !config.auto_resubscribe {
            return;
        }
        let subs: Vec<Subscription> = self
            .subscriptions
            .iter()
            .filter(|s| s.connection_id == connection_id)
            .cloned()
            .collect();
        for sub in subs {
            self.engine.subscribe(connection_id.to_string(), sub);
        }
    }

    // ── 数据工具 ──────────────────────────────────────────────────────────

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    fn push_message(&mut self, connection_id: String, record: MqttRecord) {
        let cap = self.settings.max_messages.max(100);
        let list = self.messages.entry(connection_id).or_default();
        if list.len() >= cap {
            let drop_n = list.len() - cap + 1;
            list.drain(0..drop_n);
        }
        list.push(record);
    }

    fn push_log(&mut self, entry: LogEntry) {
        if self.logs.len() >= MAX_LOGS {
            self.logs.pop_front();
        }
        self.logs.push_back(entry);
    }

    pub fn log(&mut self, conn: &str, level: LogLevel, event: &str, message: String) {
        self.push_log(LogEntry {
            timestamp: chrono::Local::now().timestamp_millis(),
            connection_id: conn.to_string(),
            level,
            event: event.to_string(),
            message,
            details: None,
        });
    }

    // ── 连接 CRUD / 生命周期 ──────────────────────────────────────────────

    pub fn save_connection(&mut self, cfg: ConnectionConfig) {
        if let Some(slot) = self.connections.iter_mut().find(|c| c.id == cfg.id) {
            *slot = cfg;
        } else {
            self.connections.push(cfg);
        }
        self.storage.save_connections(&self.connections);
    }

    pub fn duplicate_connection(&mut self, id: &str) {
        let Some(src) = self.connections.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        let mut copy = src;
        copy.id = uuid::Uuid::new_v4().to_string();
        copy.name = format!("{} 副本", copy.name);
        copy.client_id =
            format!("mqttx_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        self.connections.push(copy);
        self.storage.save_connections(&self.connections);
    }

    pub fn delete_connection(&mut self, id: &str, cx: &mut Context<Self>) {
        self.engine.close(id, false);
        self.connections.retain(|c| c.id != id);
        self.subscriptions.retain(|s| s.connection_id != id);
        self.messages.remove(id);
        self.statuses.remove(id);
        self.views.remove(id);
        self.open_tabs.retain(|t| t != id);
        if self.active_tab.as_deref() == Some(id) {
            self.active_tab = self.open_tabs.last().cloned();
        }
        self.storage.save_connections(&self.connections);
        self.storage.save_subscriptions(&self.subscriptions);
        cx.notify();
    }

    pub fn toggle_connection(&self, cfg: &ConnectionConfig) {
        if self.engine.is_connected(&cfg.id) {
            self.engine.close(&cfg.id, true);
        } else {
            self.engine.connect(cfg.clone());
        }
    }

    pub fn open_tab(&mut self, conn_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open_tabs.contains(&conn_id.to_string()) {
            self.open_tabs.push(conn_id.to_string());
            let weak = cx.entity().downgrade();
            let engine = self.engine.clone();
            let view = cx.new(|cx| ConnectionView::new(conn_id.to_string(), weak, engine, window, cx));
            self.views.insert(conn_id.to_string(), view);
        }
        self.active_tab = Some(conn_id.to_string());
        cx.notify();
    }

    fn close_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        self.open_tabs.retain(|t| t != id);
        self.views.remove(id);
        if self.active_tab.as_deref() == Some(id) {
            self.active_tab = self.open_tabs.last().cloned();
        }
        cx.notify();
    }

    // ── 订阅 ──────────────────────────────────────────────────────────────

    pub fn add_subscription(&mut self, sub: Subscription) {
        if !self
            .subscriptions
            .iter()
            .any(|s| s.connection_id == sub.connection_id && s.topic == sub.topic)
        {
            self.subscriptions.push(sub);
            self.storage.save_subscriptions(&self.subscriptions);
        }
    }

    pub fn remove_subscription(&mut self, connection_id: &str, topic: &str) {
        self.engine
            .unsubscribe(connection_id.to_string(), topic.to_string());
        self.subscriptions
            .retain(|s| !(s.connection_id == connection_id && s.topic == topic));
        self.storage.save_subscriptions(&self.subscriptions);
    }

    // ── 变量 / 预设 / 阿里云 / 设置 ──────────────────────────────────────────

    pub fn save_variables(&mut self, vars: Vec<GlobalVariable>) {
        self.variables = vars;
        self.storage.save_variables(&self.variables);
    }

    pub fn save_presets(&mut self, presets: Vec<PublishPreset>) {
        self.presets = presets;
        self.storage.save_presets(&self.presets);
    }

    /// 保存一个发布预设（按 name 去重更新）。
    pub fn upsert_publish_preset(&mut self, name: String, params: PublishParams) {
        match self.presets.iter_mut().find(|p| p.name == name) {
            Some(p) => p.params = params,
            None => self.presets.push(PublishPreset::new(name, params)),
        }
        self.storage.save_presets(&self.presets);
    }

    pub fn delete_publish_preset(&mut self, id: &str) {
        self.presets.retain(|p| p.id != id);
        self.storage.save_presets(&self.presets);
    }

    pub fn save_aliyun(&mut self, presets: Vec<AliyunPreset>) {
        self.aliyun_presets = presets;
        self.storage.save_aliyun(&self.aliyun_presets);
    }

    pub fn save_settings(&mut self, settings: AppSettings, cx: &mut App) {
        apply_theme(settings.theme, cx);
        self.settings = settings;
        self.storage.save_settings(&self.settings);
    }

    // ── 对话框 ────────────────────────────────────────────────────────────

    pub fn open_connection_form(
        &mut self,
        edit: Option<ConnectionConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.entity().downgrade();
        let engine = self.engine.clone();
        let form = cx.new(|cx| ConnectionForm::new(engine, edit, window, cx));
        window.open_dialog(cx, move |dialog, _, cx| {
            let form_ok = form.clone();
            let form_test = form.clone();
            let app_ok = app.clone();
            let testing = form_test.read(cx).is_testing();
            dialog
                .w(px(680.))
                .title("连接配置")
                .child(form.clone())
                .footer(
                    h_flex()
                        .gap_2()
                        .w_full()
                        // 测试连接常驻左下角，保存前即可验证配置
                        .child(
                            Button::new("form-test")
                                .icon(if testing {
                                    IconName::LoaderCircle
                                } else {
                                    IconName::PlugZap
                                })
                                .label(if testing { "测试中…" } else { "测试连接" })
                                .outline()
                                .loading(testing)
                                .on_click(move |_, window, cx| {
                                    form_test.update(cx, |f, cx| f.run_test(window, cx));
                                }),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("form-cancel")
                                .label("取消")
                                .outline()
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("form-ok")
                                .label("保存")
                                .primary()
                                .on_click(move |_, window, cx| {
                                    let built = form_ok.read(cx).build(cx);
                                    match built {
                                        Ok(cfg) => {
                                            app_ok
                                                .update(cx, |app, cx| {
                                                    app.save_connection(cfg);
                                                    cx.notify();
                                                })
                                                .ok();
                                            window.close_dialog(cx);
                                        }
                                        Err(e) => {
                                            window.push_notification(
                                                Notification::error(e),
                                                cx,
                                            );
                                        }
                                    }
                                }),
                        ),
                )
        });
    }
}

// ─── 主题 ────────────────────────────────────────────────────────────────────

pub fn apply_theme(pref: ThemeModePref, cx: &mut App) {
    let mode = match pref {
        ThemeModePref::Light => ThemeMode::Light,
        ThemeModePref::Dark => ThemeMode::Dark,
        // 未做系统外观检测前，“跟随系统”暂按浅色处理
        ThemeModePref::System => ThemeMode::Light,
    };
    Theme::change(mode, None, cx);
}

// ─── 渲染 ────────────────────────────────────────────────────────────────────

impl Render for MqttXApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 0.6.x 的 Root 不自动渲染 dialog/sheet/notification 层，需由内容视图挂载。
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_titlebar(window, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(self.render_sidebar(window, cx))
                    .child(self.render_main(window, cx)),
            )
            .children(gpui_kit::component::Root::render_sheet_layer(window, cx))
            .children(gpui_kit::component::Root::render_dialog_layer(window, cx))
            .children(gpui_kit::component::Root::render_notification_layer(window, cx))
    }
}

impl MqttXApp {
    fn render_titlebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app_weak = cx.entity().downgrade();

        TitleBar::new().child(
            h_flex()
                .w_full()
                .h_full()
                .items_center()
                .justify_between()
                .pl_2()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(crate::ui::logo::logo(
                            px(26.),
                            cx.theme().primary,
                            cx.theme().primary_foreground,
                        ))
                        .child(div().text_sm().font_semibold().child("MQTTX"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("MQTT 调试客户端"),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .pr_2()
                        .items_center()
                        .child(
                            Button::new("new-connection")
                                .icon(IconName::Plus)
                                .label("新建连接")
                                .primary()
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_connection_form(None, window, cx);
                                })),
                        )
                        .child(self.render_theme_toggle(cx))
                        .child(
                            Button::new("more-menu")
                                .icon(IconName::Ellipsis)
                                .ghost()
                                .small()
                                .dropdown_menu(move |menu, _, _| {
                                    let w1 = app_weak.clone();
                                    let w2 = app_weak.clone();
                                    let w3 = app_weak.clone();
                                    menu.item(
                                        PopupMenuItem::new("阿里云设备")
                                            .on_click(move |_, window, cx| {
                                                if let Some(entity) = w1.upgrade() {
                                                    crate::ui::aliyun_dialog::open(entity, window, cx);
                                                }
                                            }),
                                    )
                                    .item(PopupMenuItem::new("全局变量").on_click(
                                        move |_, window, cx| {
                                            if let Some(entity) = w2.upgrade() {
                                                crate::ui::variables_dialog::open(entity, window, cx);
                                            }
                                        },
                                    ))
                                    .separator()
                                    .item(PopupMenuItem::new("设置").on_click(
                                        move |_, window, cx| {
                                            if let Some(entity) = w3.upgrade() {
                                                crate::ui::settings_dialog::open(entity, window, cx);
                                            }
                                        },
                                    ))
                                }),
                        ),
                ),
        )
    }

    fn render_theme_toggle(&self, cx: &mut Context<Self>) -> Button {
        let dark = cx.theme().mode == ThemeMode::Dark;
        Button::new("toggle-theme")
            .icon(if dark { IconName::Sun } else { IconName::Moon })
            .ghost()
            .small()
            .tooltip(if dark { "切换为浅色" } else { "切换为深色" })
            .on_click(cx.listener(|this, _, _, cx| {
                let next = if cx.theme().mode == ThemeMode::Dark {
                    ThemeModePref::Light
                } else {
                    ThemeModePref::Dark
                };
                let mut s = this.settings.clone();
                s.theme = next;
                this.save_settings(s, cx);
                cx.notify();
            }))
    }

    fn render_sidebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).value().to_lowercase();
        let search = self.search.clone();

        let total = self.connections.len();
        let hover_bg = cx.theme().muted;
        let mut rows = v_flex().gap_0p5().flex_1().overflow_y_scrollbar();
        let mut shown = 0usize;
        for conn in self.connections.clone() {
            if !query.is_empty() && !conn.name.to_lowercase().contains(&query) {
                continue;
            }
            shown += 1;
            let status = self
                .statuses
                .get(&conn.id)
                .copied()
                .unwrap_or(ConnectionStatus::Disconnected);
            let active = self.active_tab.as_deref() == Some(conn.id.as_str());
            let cfg = conn.clone();
            let name = conn.name.clone();
            let address = conn.display_address();
            let row_id = conn.id.clone();

            rows = rows.child(
                h_flex()
                    .id(SharedString::from(format!("conn-row-{}", conn.id)))
                    .w_full()
                    .items_center()
                    .gap_2()
                    .mx_2()
                    .my_0p5()
                    .px_2()
                    .py_2()
                    .rounded_md()
                    .when(active, |this| this.bg(cx.theme().secondary))
                    .when(!active, |this| {
                        this.hover(move |this| this.bg(hover_bg))
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_tab(&row_id, window, cx);
                    }))
                    .child(
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .bg(status_color(status, cx)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .when(active, |t| t.font_semibold())
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(address),
                            ),
                    )
                    .child(self.render_conn_power(&cfg, status, cx))
                    .child(self.render_conn_menu(&conn.id, cx)),
            );
        }

        let body = if self.connections.is_empty() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .p_4()
                .child(
                    gpui_kit::component::Icon::new(IconName::PlugZap)
                        .text_color(cx.theme().muted_foreground),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("还没有连接，点击上方 + 新建"),
                )
        } else if query.is_empty() {
            v_flex().flex_1().child(rows)
        } else if shown == 0 {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .p_4()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("没有匹配的连接"),
                )
        } else {
            v_flex().flex_1().child(rows)
        };

        v_flex()
            .w(px(264.))
            .h_full()
            .flex_shrink_0()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().sidebar_foreground)
            .child(
                h_flex()
                    .h_11()
                    .px_3()
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(div().text_sm().font_semibold().child("连接"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(total.to_string()),
                            ),
                    )
                    .child(
                        Button::new("sidebar-add")
                            .icon(IconName::Plus)
                            .tooltip("新建连接")
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_connection_form(None, window, cx);
                            })),
                    ),
            )
            .child(v_flex().px_2().pb_2().child(Input::new(&search).small().prefix(
                gpui_kit::component::Icon::new(IconName::Search).small(),
            )))
            .child(body)
    }

    fn render_conn_power(
        &self,
        cfg: &ConnectionConfig,
        status: ConnectionStatus,
        cx: &mut Context<Self>,
    ) -> Button {
        let cfg = cfg.clone();
        let connected = matches!(status, ConnectionStatus::Connected);
        Button::new(SharedString::from(format!("conn-power-{}", cfg.id)))
            .icon(if connected { IconName::Square } else { IconName::PlugZap })
            .ghost()
            .xsmall()
            .tooltip(if connected { "断开" } else { "连接" })
            .on_click(cx.listener(move |this, _, _, _| {
                this.toggle_connection(&cfg);
            }))
    }

    fn render_conn_menu(&self, conn_id: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let eid = conn_id.to_string();
        Button::new(SharedString::from(format!("conn-menu-{}", conn_id)))
            .icon(IconName::Ellipsis)
            .ghost()
            .xsmall()
            .dropdown_menu(move |menu, _, _| {
                let (e1, e2, e3) = (eid.clone(), eid.clone(), eid.clone());
                let (w1, w2, w3) = (weak.clone(), weak.clone(), weak.clone());
                menu.item(PopupMenuItem::new("编辑").on_click(move |_, window, cx| {
                    w1.update(cx, |app, cx| {
                        let cfg = app.connections.iter().find(|c| c.id == e1).cloned();
                        if let Some(cfg) = cfg {
                            app.open_connection_form(Some(cfg), window, cx);
                        }
                    })
                    .ok();
                }))
                .item(PopupMenuItem::new("复制连接").on_click(move |_, _, cx| {
                    w2.update(cx, |app, cx| {
                        app.duplicate_connection(&e2);
                        cx.notify();
                    })
                    .ok();
                }))
                .separator()
                .item(PopupMenuItem::new("删除").on_click(move |_, _, cx| {
                    w3.update(cx, |app, cx| app.delete_connection(&e3, cx)).ok();
                }))
            })
    }

    fn render_main(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.open_tabs.is_empty() {
            return v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_3()
                .child(crate::ui::logo::logo(
                    px(72.),
                    cx.theme().muted,
                    cx.theme().muted_foreground,
                ))
                .child(div().text_base().font_medium().child("开始使用 MQTTX"))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .text_center()
                        .child("从左侧选择一个连接，或新建连接开始调试"),
                )
                .child(
                    Button::new("empty-new")
                        .icon(IconName::Plus)
                        .label("新建连接")
                        .outline()
                        .small()
                        .mt_1()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_connection_form(None, window, cx);
                        })),
                )
                .into_any_element();
        }

        let active = self.active_tab.clone().unwrap_or_default();
        let mut bar = TabBar::new("conn-tabs");
        for id in self.open_tabs.clone() {
            let name = self
                .connections
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| id.clone());
            let tab_id = id.clone();
            let close_id = id.clone();
            let is_active = id == active;
            bar = bar.child(
                Tab::new()
                    .label(SharedString::from(name))
                    .selected(is_active)
                    .suffix(
                        Button::new(SharedString::from(format!("tab-close-{}", id)))
                            .icon(IconName::Close)
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.close_tab(&close_id, cx);
                            })),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_tab(&tab_id, window, cx);
                    })),
            );
        }

        let view = self.views.get(&active).cloned();
        v_flex()
            .flex_1()
            .min_w(px(0.))
            .child(div().flex_shrink_0().px_2().pt_1().child(bar))
            .when_some(view, |this, v| this.child(v))
            .into_any_element()
    }
}
