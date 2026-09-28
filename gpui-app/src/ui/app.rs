//! 应用外壳：标题栏 + 连接侧边栏 + 多标签连接视图 + 引擎事件泵。

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants as _, ButtonVariant};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    h_flex, notification::Notification, v_flex, ActiveTheme as _, Selectable as _, Sizable as _,
    StyledExt as _, Theme, ThemeMode, TitleBar, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    KeyBinding, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window,
};

use crate::aliyun::AliyunPreset;
use crate::model::{
    AppSettings, ConnectionConfig, ConnectionStatus, Direction, GlobalVariable, LogEntry,
    LogLevel, MqttRecord, PublishParams, PublishPreset, SubscribeOptions, Subscription,
    ThemeModePref,
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

// 应用级快捷键动作（context=None：任意焦点状态下都匹配）。
gpui_kit::actions!(mqttx, [NewConnection, OpenSettings, ExportConnections, ImportConnections]);

/// 侧边栏分组过滤；chips 只做列表过滤，分组重命名/删除通过编辑连接的分组字段完成。
#[derive(Clone, PartialEq, Eq)]
enum GroupFilter {
    All,
    Ungrouped,
    Named(String),
}

/// 全局 action 回调只有 `&mut App`：先 defer 出当前分发栈，再取活动窗口回到实体执行，
/// 避免在窗口借用期间重入 `update`。
fn run_on_active_window(
    cx: &mut App,
    weak: gpui_kit::WeakEntity<MqttXApp>,
    f: impl FnOnce(Entity<MqttXApp>, &mut Window, &mut App) + 'static,
) {
    let Some(handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        let Some(entity) = weak.upgrade() else {
            return;
        };
        let _ = handle.update(cx, move |_, window, cx| f(entity, window, cx));
    });
}

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
    /// 分组过滤状态（chips 点击切换，与搜索条件叠加）
    group_filter: GroupFilter,
    /// 原生文件对话框进行中标记（对话框阻塞后台线程，防止重复弹出）
    file_dialog_open: bool,
    /// 按日期缓存的日志文件句柄（日期, 文件），避免每条日志都重新打开文件
    log_file: Option<(String, std::fs::File)>,
    /// 系统外观变化订阅：设置为「跟随系统」时重应用主题，随实体存活
    _appearance_obs: gpui_kit::Subscription,
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

        // 「跟随系统」：监听系统外观（深浅色）变化，仅在设置为 System 时重应用主题。
        // 订阅需随实体存活，dropped 即取消，故存入字段。
        let appearance_weak = cx.entity().downgrade();
        let appearance_obs = window.observe_window_appearance(move |window, cx| {
            let Some(app) = appearance_weak.upgrade() else {
                return;
            };
            if app.read(cx).settings.theme != ThemeModePref::System {
                return;
            }
            let mode = ThemeMode::from(window.appearance());
            Theme::change(mode, Some(window), cx);
        });

        // ── 全局快捷键：绑定 + action 注册 ──
        // 元素级 on_action 依赖焦点路径，无焦点时分发从树根开始收不到；
        // 全局 action 在冒泡末端必达，经 defer 取窗口避免在分发栈内重入。
        cx.bind_keys([
            KeyBinding::new("ctrl-n", NewConnection, None),
            KeyBinding::new("ctrl-,", OpenSettings, None),
            KeyBinding::new("ctrl-shift-e", ExportConnections, None),
            KeyBinding::new("ctrl-shift-i", ImportConnections, None),
        ]);
        let weak = cx.entity().downgrade();
        // Context 上有同名 on_action（绘制期注册），这里必须走 App 的全局注册
        App::on_action::<NewConnection>(cx, move |_, cx| {
            let w = weak.clone();
            run_on_active_window(cx, w, |entity, window, cx| {
                if window.has_active_dialog(cx) {
                    return;
                }
                entity.update(cx, |app, cx| app.open_connection_form(None, window, cx));
            });
        });
        let weak = cx.entity().downgrade();
        App::on_action::<OpenSettings>(cx, move |_, cx| {
            let w = weak.clone();
            run_on_active_window(cx, w, |entity, window, cx| {
                if window.has_active_dialog(cx) {
                    return;
                }
                crate::ui::settings_dialog::open(entity, window, cx);
            });
        });
        let weak = cx.entity().downgrade();
        App::on_action::<ExportConnections>(cx, move |_, cx| {
            let w = weak.clone();
            run_on_active_window(cx, w, |entity, window, cx| {
                // 与新建/设置一致：对话框打开时不叠加文件对话框
                if window.has_active_dialog(cx) {
                    return;
                }
                entity.update(cx, |app, cx| app.export_connections(window, cx));
            });
        });
        let weak = cx.entity().downgrade();
        App::on_action::<ImportConnections>(cx, move |_, cx| {
            let w = weak.clone();
            run_on_active_window(cx, w, |entity, window, cx| {
                if window.has_active_dialog(cx) {
                    return;
                }
                entity.update(cx, |app, cx| app.import_connections(window, cx));
            });
        });

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
            group_filter: GroupFilter::All,
            file_dialog_open: false,
            log_file: None,
            _appearance_obs: appearance_obs,
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
        // 连接成功后自动恢复已保存订阅（auto resubscribe），禁用的订阅跳过。
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
            .filter(|s| s.enabled)
            .cloned()
            .collect();
        for sub in subs {
            let opts = SubscribeOptions::from(&sub);
            self.engine
                .subscribe_with_options(connection_id.to_string(), sub, opts);
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
        // 落盘只在事件路径执行（不在 render 热路径），与内存环形缓冲互不影响
        self.write_log_file(&entry);
        if self.logs.len() >= MAX_LOGS {
            self.logs.pop_front();
        }
        self.logs.push_back(entry);
    }

    /// 追加写入按日切分的日志文件 `mqttx-YYYY-MM-DD.log`。
    /// 文件句柄按日期缓存复用，只有跨日或写失败后才重新打开；
    /// 失败只在首次 eprintln 一次后保持静默（成功一次即复位，便于恢复后再次提示），
    /// 且绝不回调 log/push_log（防止递归触发日志）。
    fn write_log_file(&mut self, entry: &LogEntry) {
        use std::sync::atomic::{AtomicBool, Ordering};

        static WRITE_FAILED: AtomicBool = AtomicBool::new(false);

        match self.append_log_line(entry) {
            Ok(()) => WRITE_FAILED.store(false, Ordering::Relaxed),
            Err(e) => {
                // 关句柄置空，下次写入自动重开重试
                self.log_file = None;
                if !WRITE_FAILED.swap(true, Ordering::Relaxed) {
                    eprintln!("[app] 写入日志文件失败（后续静默）: {e}");
                }
            }
        }
    }

    /// 打开（或复用按日缓存的）日志文件并追加一行。
    fn append_log_line(&mut self, entry: &LogEntry) -> std::io::Result<()> {
        use std::io::Write as _;
        use chrono::TimeZone as _;

        let ts = chrono::Local
            .timestamp_millis_opt(entry.timestamp)
            .single()
            .unwrap_or_else(chrono::Local::now);
        let day = ts.format("%Y-%m-%d").to_string();
        let need_reopen = match &self.log_file {
            Some((cached_day, _)) => *cached_day != day,
            None => true,
        };
        if need_reopen {
            let dir = self.storage.log_dir();
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(format!("mqttx-{day}.log"));
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            self.log_file = Some((day, file));
        }
        // 连接名查不到时回退为 id，保证行格式恒定
        let conn = self
            .connections
            .iter()
            .find(|c| c.id == entry.connection_id)
            .map(|c| c.name.as_str())
            .unwrap_or(entry.connection_id.as_str());
        let file = &mut self
            .log_file
            .as_mut()
            .expect("上方已确保日志句柄存在")
            .1;
        writeln!(
            file,
            "{} [{}] {}/{}: {}",
            ts.format("%Y-%m-%d %H:%M:%S%.3f"),
            entry.level.label(),
            conn,
            entry.event,
            entry.message
        )
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
        self.errors.remove(id);
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
            // 遗嘱支持 {{变量}}：对副本注入，保存的配置仍为模板原文
            let mut cfg = cfg.clone();
            crate::model::render_will_templates(&mut cfg, &self.variables);
            self.engine.connect(cfg);
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

    /// 数据目录（设置对话框展示 / 打开用）。
    pub fn data_dir(&self) -> PathBuf {
        self.storage.dir().to_path_buf()
    }

    /// 日志目录（设置对话框展示 / 打开用）。
    pub fn log_dir(&self) -> PathBuf {
        self.storage.log_dir()
    }

    // ── 连接导入 / 导出 ────────────────────────────────────────────────────

    /// 在后台线程弹出阻塞式原生文件对话框，完成后回主线程执行 `done`。
    /// 对话框进行中置位，防止重复弹出。
    fn spawn_file_dialog<R, F, D>(
        &mut self,
        work: F,
        done: D,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
        D: FnOnce(&mut Self, R, &mut Window, &mut Context<Self>) + 'static,
    {
        if self.file_dialog_open {
            return;
        }
        self.file_dialog_open = true;
        // rfd 的阻塞对话框不可占用 UI 线程：结果经 smol 通道送回异步任务
        let (tx, rx) = smol::channel::bounded::<R>(1);
        std::thread::spawn(move || {
            let _ = tx.send_blocking(work());
        });
        let weak = cx.entity().downgrade();
        cx.spawn_in(window, async move |_this, cx: &mut gpui_kit::AsyncWindowContext| {
            let result = rx.recv().await.ok();
            weak.update_in(cx, |app, window, cx| {
                app.file_dialog_open = false;
                if let Some(result) = result {
                    done(app, result, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// 导出全部连接为 JSON（原生保存对话框，默认文件名 connections-export.json）。
    pub fn export_connections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.storage.dir().to_path_buf();
        self.spawn_file_dialog(
            move || {
                rfd::FileDialog::new()
                    .set_title("导出连接")
                    .set_directory(dir)
                    .set_file_name("connections-export.json")
                    .add_filter("JSON", &["json"])
                    .save_file()
            },
            |app, path, window, cx| {
                let Some(path) = path else {
                    return; // 用户取消
                };
                let count = app.connections.len();
                match crate::store::export_connections_to(&path, &app.connections) {
                    Ok(()) => window.push_notification(
                        Notification::success(format!(
                            "已导出 {count} 条连接到 {}",
                            path.display()
                        )),
                        cx,
                    ),
                    Err(e) => window.push_notification(Notification::error(e), cx),
                }
            },
            window,
            cx,
        );
    }

    /// 导入连接：按 id 去重（同 id 跳过），新 id 保留文件原值；读取解析在后台线程完成。
    pub fn import_connections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.storage.dir().to_path_buf();
        self.spawn_file_dialog(
            move || {
                let Some(path) = rfd::FileDialog::new()
                    .set_title("导入连接")
                    .set_directory(dir)
                    .add_filter("JSON", &["json"])
                    .pick_file()
                else {
                    return Ok(None); // 用户取消
                };
                crate::store::import_connections_from(&path).map(Some)
            },
            |app, result, window, cx| match result {
                Err(e) => {
                    window.push_notification(Notification::error(e), cx);
                }
                Ok(None) => {}
                Ok(Some(items)) => {
                    let mut existing: std::collections::HashSet<String> =
                        app.connections.iter().map(|c| c.id.clone()).collect();
                    let mut added = 0usize;
                    let mut skipped = 0usize;
                    for item in items {
                        if existing.contains(&item.id) {
                            skipped += 1;
                        } else {
                            // 记录已入库 id，导入文件内部的重复 id 也只收一条
                            existing.insert(item.id.clone());
                            app.connections.push(item);
                            added += 1;
                        }
                    }
                    if added > 0 {
                        app.storage.save_connections(&app.connections);
                    }
                    window.push_notification(
                        Notification::success(format!("导入 {added} 条，跳过 {skipped} 条")),
                        cx,
                    );
                }
            },
            window,
            cx,
        );
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
        // 表单的分组下拉需要现有分组列表（去重、排序）
        let mut groups: Vec<String> = self
            .connections
            .iter()
            .filter_map(|c| c.group.as_ref().map(|g| g.trim().to_string()))
            .filter(|g| !g.is_empty())
            .collect();
        groups.sort();
        groups.dedup();
        let form = cx.new(|cx| ConnectionForm::new(engine, edit, groups, window, cx));
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
                                            // 已连接的连接改了关键参数，重连后才生效，先算好再保存
                                            let mut stale = false;
                                            app_ok
                                                .update(cx, |app, cx| {
                                                    let connected = app.statuses.get(&cfg.id)
                                                        == Some(&ConnectionStatus::Connected);
                                                    if connected
                                                        && app
                                                            .connections
                                                            .iter()
                                                            .find(|c| c.id == cfg.id)
                                                            .is_some_and(|old| {
                                                                old.session_params_changed(&cfg)
                                                            })
                                                    {
                                                        stale = true;
                                                    }
                                                    app.save_connection(cfg);
                                                    cx.notify();
                                                })
                                                .ok();
                                            window.close_dialog(cx);
                                            if stale {
                                                window.push_notification(
                                                    Notification::info("配置已保存，重连后生效"),
                                                    cx,
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            // 触发重绘，让字段级红字即时显示（错误已写入表单内部）
                                            form_ok.update(cx, |_, cx| cx.notify());
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

/// 应用主题偏好。
///
/// 「跟随系统」读取 gpui 的窗口外观（`App::window_appearance`）；系统深浅色变化由
/// `MqttXApp::new` 里注册的 `Window::observe_window_appearance` 监听并重应用本函数。
pub fn apply_theme(pref: ThemeModePref, cx: &mut App) {
    let mode = match pref {
        ThemeModePref::Light => ThemeMode::Light,
        ThemeModePref::Dark => ThemeMode::Dark,
        ThemeModePref::System => ThemeMode::from(cx.window_appearance()),
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
                                    let w4 = app_weak.clone();
                                    let w5 = app_weak.clone();
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
                                    .item(PopupMenuItem::new("导出连接").on_click(
                                        move |_, window, cx| {
                                            if let Some(entity) = w4.upgrade() {
                                                entity.update(cx, |app, cx| {
                                                    app.export_connections(window, cx)
                                                });
                                            }
                                        },
                                    ))
                                    .item(PopupMenuItem::new("导入连接").on_click(
                                        move |_, window, cx| {
                                            if let Some(entity) = w5.upgrade() {
                                                entity.update(cx, |app, cx| {
                                                    app.import_connections(window, cx)
                                                });
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
        // 分组被改名/删除后过滤器可能悬空（列表恒空），渲染前复位到「全部」
        let dangling = matches!(&self.group_filter, GroupFilter::Named(name)
            if !self.connections.iter().any(|c| {
                c.group.as_deref().map(str::trim) == Some(name.as_str())
            }));
        if dangling {
            self.group_filter = GroupFilter::All;
        }
        let filter = self.group_filter.clone();

        let total = self.connections.len();
        let hover_bg = cx.theme().muted;
        let mut rows = v_flex().gap_0p5().flex_1().overflow_y_scrollbar();
        let mut shown = 0usize;
        for conn in self.connections.clone() {
            // 分组过滤与搜索叠加（AND）
            let group = conn.group.as_deref().map(str::trim).unwrap_or("");
            let match_group = match &filter {
                GroupFilter::All => true,
                GroupFilter::Ungrouped => group.is_empty(),
                GroupFilter::Named(name) => group == name.as_str(),
            };
            if !match_group {
                continue;
            }
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
            .child(self.render_group_chips(cx))
            .child(body)
    }

    /// 搜索框下方的分组 chips：全部 / 未分组 / 各分组（带计数徽标），点击切换过滤。
    fn render_group_chips(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut names: Vec<String> = self
            .connections
            .iter()
            .filter_map(|c| c.group.as_deref().map(str::trim))
            .filter(|g| !g.is_empty())
            .map(str::to_string)
            .collect();
        names.sort();
        names.dedup();

        let ungrouped = self
            .connections
            .iter()
            .filter(|c| c.group.as_deref().map(str::trim).unwrap_or("").is_empty())
            .count();

        // 分组多时限制高度并纵向滚动，避免 chips 换行挤占下方连接列表
        let mut row = h_flex()
            .gap_1()
            .flex_wrap()
            .px_2()
            .pb_1()
            .max_h(px(88.))
            .overflow_y_scrollbar();
        row = row.child(self.render_group_chip(
            "chip-all",
            format!("全部 {}", self.connections.len()),
            GroupFilter::All,
            cx,
        ));
        row = row.child(self.render_group_chip(
            "chip-ungrouped",
            format!("未分组 {ungrouped}"),
            GroupFilter::Ungrouped,
            cx,
        ));
        for name in names {
            let count = self
                .connections
                .iter()
                .filter(|c| c.group.as_deref().map(str::trim) == Some(name.as_str()))
                .count();
            row = row.child(self.render_group_chip(
                SharedString::from(format!("chip-{}", name)),
                format!("{name} {count}"),
                GroupFilter::Named(name),
                cx,
            ));
        }
        row
    }

    fn render_group_chip(
        &self,
        id: impl Into<gpui_kit::ElementId>,
        label: String,
        filter: GroupFilter,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = match (&self.group_filter, &filter) {
            (GroupFilter::All, GroupFilter::All)
            | (GroupFilter::Ungrouped, GroupFilter::Ungrouped) => true,
            (GroupFilter::Named(a), GroupFilter::Named(b)) => a == b,
            _ => false,
        };
        div()
            .id(id)
            .px_2()
            .py_0p5()
            .rounded_md()
            .text_xs()
            .when(selected, |d| d.bg(cx.theme().secondary).font_semibold())
            .when(!selected, |d| {
                d.bg(cx.theme().muted)
                    .text_color(cx.theme().muted_foreground)
                    .hover(|d| d.bg(cx.theme().secondary))
            })
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                // listener 是 Fn（可多次调用），只能克隆而不能移出捕获的 filter
                this.group_filter = filter.clone();
                cx.notify();
            }))
            .child(label)
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
                .item(PopupMenuItem::new("删除").on_click(move |_, window, cx| {
                    let Some(app) = w3.upgrade() else {
                        return;
                    };
                    // 二次确认：删除会级联清理订阅并断开连接，不可撤销
                    let (name, sub_count) = {
                        let a = app.read(cx);
                        let name = a
                            .connections
                            .iter()
                            .find(|c| c.id == e3)
                            .map(|c| c.name.clone())
                            .unwrap_or_else(|| e3.clone());
                        let n = a.subscriptions.iter().filter(|s| s.connection_id == e3).count();
                        (name, n)
                    };
                    let confirm_id = e3.clone();
                    window.open_alert_dialog(cx, move |alert, _, _| {
                        // 构建闭包是 Fn（可重复调用），克隆后再移入按钮回调
                        let app = app.clone();
                        let confirm_id = confirm_id.clone();
                        alert
                            .title("删除连接")
                            .description(SharedString::from(format!(
                                "确定删除「{name}」？将同时删除 {sub_count} 个订阅，并断开当前连接，此操作不可撤销。"
                            )))
                            .button_props(
                                DialogButtonProps::default()
                                    .ok_text("删除")
                                    .ok_variant(ButtonVariant::Danger)
                                    .show_cancel(true),
                            )
                            .on_ok(move |_, _, cx| {
                                app.update(cx, |app, cx| app.delete_connection(&confirm_id, cx));
                                true
                            })
                    });
                }))
            })
    }

    fn render_main(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.open_tabs.is_empty() {
            // 父级 h_flex 交叉轴居中子项，容器必须 h_full 占满高度才不会整体下沉
            return v_flex()
                .flex_1()
                .h_full()
                .min_h(px(0.))
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
        let mut bar = TabBar::new("conn-tabs")
            // 连接名过长时截断省略，避免单个页签占满整行
            .max_width(px(180.));
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
        // h_full：父级 h_flex 交叉轴居中子项，不占满高度会被整体垂直居中；
        // 视图根是 size_full，须包在 flex_1 容器里，否则会盖住 TabBar 并溢出。
        v_flex()
            .flex_1()
            .h_full()
            .min_h(px(0.))
            .min_w(px(0.))
            .child(div().flex_shrink_0().px_2().pt_1().child(bar))
            .when_some(view, |this, v| {
                this.child(v_flex().flex_1().min_h(px(0.)).child(v))
            })
            .into_any_element()
    }
}
