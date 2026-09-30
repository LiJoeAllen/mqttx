//! 应用外壳：标题栏 + 连接侧边栏 + 多标签连接视图 + 引擎事件泵。
//!
//! - [`events`] 引擎事件 → 应用状态（连接状态、消息、日志）
//! - [`actions`] 用户动作（连接/订阅/预设/设置等状态写操作）
//! - [`io`] 连接配置导入导出
//! - [`theme`] 主题应用
//! - [`titlebar`] / [`sidebar`] / [`main_area`] 渲染分区
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::input::InputState;
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{
    h_flex, notification::Notification, v_flex, ActiveTheme as _, Selectable as _, Sizable as _,
    StyledExt as _, Theme, ThemeMode, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    KeyBinding, ParentElement as _, Render, StatefulInteractiveElement as _,
    Styled as _, Window,
};

use crate::aliyun::AliyunPreset;
use crate::model::{
    AppSettings, ConnectionConfig, ConnectionStatus, GlobalVariable, LogEntry, MessageRing, PublishPreset,
    Subscription, ThemeModePref,
};
use crate::mqtt::MqttEngine;
use crate::store::{Storage, UiState};
use crate::update;
use crate::ui::connection_view::ConnectionView;

mod actions;
mod events;
mod io;
mod main_area;
mod sidebar;
mod theme;
mod titlebar;

use theme::apply_theme;

const MAX_LOGS: usize = 3000;
/// 引擎 → UI 的事件队列容量（有界，见 `MqttEngine::emit` 的丢弃策略）
const EVENT_QUEUE_CAPACITY: usize = 1024;

// 应用级快捷键动作（context=None：任意焦点状态下都匹配）。
gpui_kit::actions!(mqttx, [NewConnection, OpenSettings, ExportConnections, ImportConnections, OpenResourceMonitor, PublishMessage]);

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
    /// 每连接消息环形缓冲：键为驻留后的连接 ID，条数 + 字节预算双重约束
    pub messages: HashMap<Arc<str>, MessageRing>,
    /// Arc 共享：日志面板每帧最多克隆 3000 条，深拷贝会让渲染线程每秒
    /// 产生数十万次 String 堆分配；改 Arc 后仅增加引用计数
    pub logs: VecDeque<Arc<LogEntry>>,
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
    log_file: Option<(String, std::io::BufWriter<std::fs::File>)>,
    /// 系统外观变化订阅：设置为「跟随系统」时重应用主题，随实体存活
    _appearance_obs: gpui_kit::Subscription,
    /// 窗口置顶状态（标题栏钉子按钮切换）
    pinned: bool,
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

        // 有界事件队列：引擎侧对高频事件（消息/日志）做丢弃并计数，保证 UI 消费
        // 不过来时内存有上界。1024 × 单条最大 128KB ≈ 最坏 128MB，常规消息下几百 KB。
        let (tx, rx) = smol::channel::bounded(EVENT_QUEUE_CAPACITY);
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

        // OTA：清理上次自更新遗留文件；开启自动检查则后台静默检查并下载，
        // 下载完成后弹对话框征得同意再安装（不自动重启）。
        update::cleanup_old();
        // 自动连接：勾选「启动时自动连接」的连接逐个拉起（引擎异步建连）
        for cfg in connections
            .iter()
            .filter(|c| c.auto_connect)
            .cloned()
            .collect::<Vec<_>>()
        {
            let mut cfg = cfg;
            crate::model::render_will_templates(&mut cfg, &variables);
            engine.connect(cfg);
        }

        // 会话恢复：载入上次打开的标签页（过滤掉已删除的连接）。
        // 若无可恢复标签且启用了自动连接，则自动打开第一个自动连接的工作区，
        // 避免「连接已在后台连上、主区却演空态」的不连贯。
        let ui_state = storage.load_ui_state();
        let conn_ids: std::collections::HashSet<String> =
            connections.iter().map(|c| c.id.clone()).collect();
        let restored_tabs: Vec<String> = ui_state
            .open_tabs
            .into_iter()
            .filter(|id| conn_ids.contains(id))
            .collect();
        let restored_active = ui_state
            .active_tab
            .filter(|id| restored_tabs.contains(id));
        let auto_first = connections
            .iter()
            .find(|c| c.auto_connect)
            .map(|c| c.id.clone());
        let defer_tabs = restored_tabs.clone();
        let defer_active = restored_active.clone();
        cx.defer_in(window, move |app, window, cx| {
            if defer_tabs.is_empty() {
                if let Some(id) = auto_first {
                    app.open_tab(&id, window, cx);
                }
                return;
            }
            for id in defer_tabs {
                app.open_tab(&id, window, cx);
            }
            if let Some(id) = defer_active {
                app.active_tab = Some(id);
            }
            cx.notify();
        });
        if settings.auto_check_update {
            let rx = engine.run_blocking(update::check_and_download);
            let weak = cx.entity().downgrade();
            cx.spawn_in(window, async move |_this, cx: &mut gpui_kit::AsyncWindowContext| {
                match rx.recv().await {
                    Ok(Ok(Some(staged))) => {
                        weak.update_in(cx, |_app, window, cx| {
                        window.open_dialog(cx, move |dialog, _, _cx| {
                            let staged = staged.clone();
                            let staged_skip = staged.clone();
                            let staged_consent = staged.clone();
                            dialog
                                    .w(px(440.))
                                    .title("更新就绪")
                                    .child(
                                        v_flex().gap_2().child(
                                            div().text_sm().child(format!(
                                                "新版本 v{} 已下载完成（已通过 sha256 校验）。",
                                                staged.version
                                            )),
                                        ),
                                    )
                                    .footer(
                                        h_flex()
                                            .gap_2()
                                            .justify_end()
                                            .w_full()
                                            .child(
                                                Button::new("upd-skip")
                                                    .label("暂不更新")
                                                    .outline()
                                                    .on_click(move |_, window, cx| {
                                                        // 拒绝本次更新：删除暂存包，
                                                        // 下次启动不会被自动安装
                                                        update::discard_staged(&staged_skip);
                                                        window.close_dialog(cx);
                                                    }),
                                            )
                                            .child(
                                                Button::new("upd-later")
                                                    .label("下次启动安装")
                                                    .outline()
                                                    .on_click(move |_, window, cx| {
                                                        // 同意标记：启动时据此自动安装；
                                                        // 用户跳过对话框则不会强制升级
                                                        update::mark_install_consent(&staged_consent);
                                                        window.close_dialog(cx);
                                                    }),
                                            )
                                            .child(
                                                Button::new("upd-now")
                                                    .label("立即安装")
                                                    .primary()
                                                    .on_click(move |_, window, cx| {
                                                        window.close_dialog(cx);
                                                        if let Err(e) =
                                                            update::install_staged(&staged)
                                                        {
                                                            window.push_notification(
                                                                Notification::error(format!(
                                                                    "安装失败：{e}"
                                                                )),
                                                                cx,
                                                            );
                                                        } else {
                                                            cx.quit();
                                                        }
                                                    }),
                                            ),
                                    )
                            });
                        })
                        .ok();
                    }
                    // 检查/下载失败不打扰用户，但也不能完全无声
                    Ok(Err(e)) => {
                        eprintln!("[app] 启动更新检查失败: {e}");
                        sentry::capture_message(
                            &format!("startup update check failed: {e}"),
                            sentry::Level::Warning,
                        );
                    }
                    _ => {}
                }
            })
            .detach();
        }

        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索连接…"));

        // 自验后门：MQTTX_OPEN_RESMON=1 启动时直接打开资源监控
        // （与 MQTTX_VERSION_OVERRIDE 同类的测试入口，供自动化验证 UI）。
        // defer 到 effects 阶段执行，避免在 MqttXApp 构造借用内嵌套打开
        if std::env::var("MQTTX_OPEN_RESMON").as_deref() == Ok("1") {
            cx.defer_in(window, move |_, window, cx| {
                crate::ui::dialogs::resource_dialog::open(cx.entity(), window, cx);
            });
        }

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
            // Theme::change 只更新全局令牌，不保证整窗重绘；未损坏区域会
            // 滞留旧主题像素（实测发布栏长期不收敛），强制重画一次
            window.refresh();
        });

        // ── 全局快捷键：绑定 + action 注册 ──
        // 元素级 on_action 依赖焦点路径，无焦点时分发从树根开始收不到；
        // 全局 action 在冒泡末端必达，经 defer 取窗口避免在分发栈内重入。
        cx.bind_keys([
            KeyBinding::new("ctrl-n", NewConnection, None),
            KeyBinding::new("ctrl-,", OpenSettings, None),
            KeyBinding::new("ctrl-shift-e", ExportConnections, None),
            KeyBinding::new("ctrl-shift-i", ImportConnections, None),
            KeyBinding::new("ctrl-shift-r", OpenResourceMonitor, None),
            KeyBinding::new("ctrl-enter", PublishMessage, None),
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
                crate::ui::dialogs::settings_dialog::open(entity, window, cx);
            });
        });
        let weak = cx.entity().downgrade();
        App::on_action::<OpenResourceMonitor>(cx, move |_, cx| {
            let w = weak.clone();
            run_on_active_window(cx, w, |entity, window, cx| {
                if window.has_active_dialog(cx) {
                    return;
                }
                crate::ui::dialogs::resource_dialog::open(entity, window, cx);
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
        // Ctrl+Enter 全局发送：焦点在负载框、QoS 下拉或任意位置都能发送，
        // 不再依赖焦点恰好落在某个输入框上（无标签页时静默忽略）
        let weak = cx.entity().downgrade();
        App::on_action::<PublishMessage>(cx, move |_, cx| {
            let w = weak.clone();
            run_on_active_window(cx, w, |entity, window, cx| {
                if window.has_active_dialog(cx) {
                    return;
                }
                entity.update(cx, |app, cx| {
                    if let Some(tab) = app.active_tab.clone()
                        && let Some(view) = app.views.get(&tab).cloned()
                    {
                        view.update(cx, |view, cx| view.do_publish(window, cx));
                    }
                });
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
            // 标签恢复统一走下方 defer 的 open_tab（负责创建视图实体），
            // 这里不能预填 open_tabs：预填会让 open_tab 误判标签已存在而跳过建视图
            open_tabs: Vec::new(),
            active_tab: None,
            views: HashMap::new(),
            search,
            group_filter: GroupFilter::All,
            file_dialog_open: false,
            log_file: None,
            _appearance_obs: appearance_obs,
            pinned: false,
        }
    }

    // ── 引擎事件 ──────────────────────────────────────────────────────────

}

impl Render for MqttXApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 0.7 起 Root 自动渲染 dialog/sheet/notification 层，内容视图无需再手动挂载。
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
    }
}
