//! 资源监控对话框：软件自身的 CPU / 内存 / 线程 / 句柄占用，
//! 以及每连接的消息内存预算使用情况。
//!
//! 交互：右上角可暂停/恢复自动刷新（默认开启，1.5s），并可手动立即刷新；
//! 全局快捷键 Ctrl+Shift+R 打开；`MQTTX_OPEN_RESMON=1` 启动时直开（自动化验证）。

use std::time::Duration;

use gpui_kit::component::button::Button;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _, StyledExt as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window,
};

use crate::model::MAX_CONNECTION_MESSAGE_BYTES;
use crate::sysmon::{Sample, Sampler};
use crate::ui::app::MqttXApp;
use crate::ui::IconName;

const REFRESH_INTERVAL: Duration = Duration::from_millis(1500);

struct ConnStat {
    name: String,
    address: String,
    status: &'static str,
    connected: bool,
    messages: usize,
    bytes: usize,
}

pub struct ResourceMonitorDialog {
    app: Entity<MqttXApp>,
    sampler: Sampler,
    sample: Sample,
    conns: Vec<ConnStat>,
    logs: usize,
    tabs: usize,
    total_messages: usize,
    auto_refresh: bool,
    /// 首次 render 时才做初始采样：构造发生在 MqttXApp::new 的借用中，
    /// 过早 read app 会触发双重借用 panic
    first_render: bool,
}

impl ResourceMonitorDialog {
    fn new(app: Entity<MqttXApp>, cx: &mut Context<Self>) -> Self {
        Self {
            app,
            sampler: Sampler::new(),
            sample: Sample::default(),
            conns: Vec::new(),
            logs: 0,
            tabs: 0,
            total_messages: 0,
            auto_refresh: true,
            first_render: true,
        }
    }

    /// 采样进程资源 + 快照每连接的消息内存占用。
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = self.sampler.sample() {
            self.sample = s;
        }
        let app = self.app.read(cx);
        self.logs = app.logs.len();
        self.tabs = app.open_tab_count();
        self.total_messages = 0;
        self.conns = app
            .connections
            .iter()
            .map(|c| {
                let ring = app.messages.get(c.id.as_str());
                let (messages, bytes) = ring
                    .map(|m| (m.len(), m.retained_bytes()))
                    .unwrap_or((0, 0));
                self.total_messages += messages;
                ConnStat {
                    name: c.name.clone(),
                    address: c.display_address(),
                    status: app
                        .statuses
                        .get(&c.id)
                        .copied()
                        .map(|s| s.label())
                        .unwrap_or("未连接"),
                    connected: app.engine.is_connected(&c.id),
                    messages,
                    bytes,
                }
            })
            .collect();
    }

    fn fmt_bytes(bytes: u64) -> String {
        if bytes >= 1024 * 1024 {
            format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
        } else if bytes >= 1024 {
            format!("{:.1} KB", bytes as f64 / 1024.)
        } else {
            format!("{bytes} B")
        }
    }

    /// 指标卡片：图标 + 标签行、大号数值、可选副文本与进度条。
    #[allow(clippy::too_many_arguments)]
    fn tile(
        &self,
        id: &'static str,
        icon: IconName,
        label: &str,
        value: String,
        sub: Option<String>,
        progress: Option<f32>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .flex_1()
            .min_w(px(248.))
            .gap_1p5()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted.alpha(0.25))
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(Icon::new(icon).size_4().text_color(muted))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(label.to_string()),
                    ),
            )
            .child(div().text_xl().font_semibold().child(value))
            .when_some(sub, |t, s| {
                t.child(div().text_xs().text_color(muted).child(s))
            })
            .when_some(progress, |t, p| {
                t.child(Progress::new(id).value(p.clamp(0., 100.)).small())
            })
    }

    fn section_header(&self, icon: IconName, title: &str, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_1p5()
            .items_center()
            .child(Icon::new(icon).size_4().text_color(cx.theme().accent))
            .child(
                div()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child(title.to_string()),
            )
    }

    fn conn_card(
        &self,
        index: usize,
        c: &ConnStat,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let pct = (c.bytes as f32 / MAX_CONNECTION_MESSAGE_BYTES as f32 * 100.).min(100.);
        v_flex()
            .gap_1p5()
            .p_2p5()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(crate::ui::logo::logo(
                                px(16.),
                                cx.theme().primary,
                                cx.theme().primary_foreground,
                            ))
                            .child(div().text_sm().font_semibold().child(c.name.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(c.address.clone()),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                Icon::new(if c.connected {
                                    IconName::Zap
                                } else {
                                    IconName::Clock
                                })
                                .size_3p5()
                                .text_color(if c.connected {
                                    cx.theme().accent
                                } else {
                                    muted
                                }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_full()
                                    .when(c.connected, |d| {
                                        d.bg(cx.theme().accent.alpha(0.12))
                                            .text_color(cx.theme().accent)
                                    })
                                    .when(!c.connected, |d| {
                                        d.bg(cx.theme().muted).text_color(muted)
                                    })
                                    .child(c.status.to_string()),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(Icon::new(IconName::List).size_3p5().text_color(muted))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{} 条消息", c.messages)),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                Icon::new(IconName::Database)
                                    .size_3p5()
                                    .text_color(muted),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!(
                                        "预算占用 {:.0}%（{} / {}）",
                                        pct,
                                        Self::fmt_bytes(c.bytes as u64),
                                        Self::fmt_bytes(MAX_CONNECTION_MESSAGE_BYTES as u64)
                                    )),
                            ),
                    ),
            )
            .child(
                Progress::new(SharedString::from(format!("conn-mem-{index}")))
                    .value(pct)
                    .small(),
            )
    }
}

impl Render for ResourceMonitorDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.first_render {
            self.first_render = false;
            self.refresh(cx);
        }
        let s = &self.sample;
        let up = crate::sysmon::uptime();
        let uptime_text = if up.as_secs() >= 3600 {
            format!("{}h{}m", up.as_secs() / 3600, (up.as_secs() % 3600) / 60)
        } else {
            format!("{}m{}s", up.as_secs() / 60, up.as_secs() % 60)
        };
        let muted = cx.theme().muted_foreground;
        let connected_count = self.conns.iter().filter(|c| c.connected).count();
        let cpu = s.cpu_percent;

        // ── 头部：标题 + 运行时长 + 刷新控制 ──
        let header = h_flex()
            .justify_between()
            .items_center()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(crate::ui::logo::logo(
                        px(22.),
                        cx.theme().primary,
                        cx.theme().primary_foreground,
                    ))
                    .child(div().text_base().font_semibold().child("资源监控"))
                    .child(
                        div()
                            .text_xs()
                            .px_2()
                            .py_0p5()
                            .rounded_full()
                            .bg(cx.theme().muted)
                            .text_color(muted)
                            .child(format!("运行 {uptime_text}")),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child("自动刷新"),
                            )
                            .child(
                                Switch::new("resmon-auto")
                                    .checked(self.auto_refresh)
                                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                                        this.auto_refresh = *v;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        Button::new("resmon-refresh")
                            .icon(IconName::RefreshCw)
                            .label("立即刷新")
                            .outline()
                            .xsmall()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh(cx);
                                cx.notify();
                            })),
                    ),
            );

        // ── 指标卡片 2×2 ──
        let ws = s.working_set;
        let peak = s.peak_working_set.max(1);
        let tiles = h_flex()
            .flex_wrap()
            .gap_2()
            .child(self.tile(
                "tile-cpu",
                IconName::Cpu,
                "CPU 占用",
                format!("{cpu:.1}%"),
                Some("按可用核数归一".into()),
                Some(cpu),
                cx,
            ))
            .child(self.tile(
                "tile-ws",
                IconName::MemoryStick,
                "工作集内存",
                Self::fmt_bytes(ws),
                Some(format!("峰值 {}", Self::fmt_bytes(s.peak_working_set))),
                Some(ws as f32 / peak as f32 * 100.),
                cx,
            ))
            .child(self.tile(
                "tile-private",
                IconName::HardDrive,
                "提交内存",
                Self::fmt_bytes(s.private_bytes),
                Some("进程独占的物理页承诺".into()),
                None,
                cx,
            ))
            .child(self.tile(
                "tile-threads",
                IconName::Layers,
                "线程 · 句柄",
                format!("{} / {}", s.threads, s.handles),
                Some("tokio 引擎 + GPUI 渲染".into()),
                None,
                cx,
            ));

        // ── 连接卡片 ──
        let mut conns_block = v_flex().gap_2();
        if self.conns.is_empty() {
            conns_block = conns_block.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(Icon::new(IconName::Info).size_4().text_color(muted))
                    .child(div().text_sm().text_color(muted).child("暂无连接")),
            );
        }
        for (i, c) in self.conns.iter().enumerate() {
            conns_block = conns_block.child(self.conn_card(i, c, cx));
        }

        v_flex()
            .gap_3()
            .w(px(600.))
            .child(header)
            .child(tiles)
            .child(
                v_flex()
                    .gap_2()
                    .child(self.section_header(IconName::Database, "消息与连接", cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!(
                                "{} 个连接（已连接 {}）· 打开标签 {} · 内存消息 {} 条（每连接上限 {} 条）· 日志 {} 条",
                                self.conns.len(),
                                connected_count,
                                self.tabs,
                                self.total_messages,
                                self.app.read(cx).settings.max_messages,
                                self.logs
                            )),
                    )
                    .child(conns_block),
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(Icon::new(IconName::Tag).size_3p5().text_color(muted))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(if self.auto_refresh {
                                format!(
                                    "版本 v{} · 每 1.5 秒自动刷新",
                                    crate::update::current_version()
                                )
                            } else {
                                format!(
                                    "版本 v{} · 自动刷新已暂停",
                                    crate::update::current_version()
                                )
                            }),
                    ),
            )
    }
}

pub fn open(app: Entity<MqttXApp>, window: &mut Window, cx: &mut App) {
    let dialog_view = cx.new(|cx| ResourceMonitorDialog::new(app, cx));
    // 用弱引用刷新：对话框关闭、实体释放后 update 返回 Err，循环退出。
    // 强引用的 update 在实体释放后会 panic。
    let weak_view = dialog_view.downgrade();
    cx.spawn(async move |cx: &mut gpui_kit::AsyncApp| {
        loop {
            cx.background_executor()
                .timer(REFRESH_INTERVAL)
                .await;
            if weak_view
                .update(cx, |d, cx| {
                    if d.auto_refresh {
                        d.refresh(cx);
                        cx.notify();
                    }
                })
                .is_err()
            {
                break;
            }
        }
    })
    .detach();

    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .w(px(640.))
            .title("资源监控")
            .child(dialog_view.clone())
            .footer(
                h_flex()
                    .justify_end()
                    .w_full()
                    .child(
                        Button::new("res-close")
                            .label("关闭")
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    });
}
