//! 应用设置对话框：主题、消息缓存、时间戳、自动检查更新、数据/日志目录、关于。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    h_flex, notification::Notification, v_flex, ActiveTheme as _, Sizable as _, StyledExt as _,
    WindowExt as _,
};
use gpui_kit::{
    div, px, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Window,
};

use crate::model::{AppSettings, ThemeModePref, MAX_CONNECTION_MESSAGE_BYTES};
use crate::mqtt::MqttEngine;
use crate::ui::app::MqttXApp;
use crate::ui::widgets::{field, make_select, OptionDelegate};
use crate::ui::IconName;
use crate::update::{self, UpdateInfo};

const THEMES: [&str; 3] = ["跟随系统（暂按浅色）", "浅色", "深色"];

/// 消息缓存条数的合法范围：太少不够回看，太多占内存
const MAX_MESSAGES_MIN: usize = 100;
const MAX_MESSAGES_MAX: usize = 100_000;

const SITE_URL: &str = "https://mqttx.app";
const REPO_URL: &str = "https://gitea.heavenlybook.cn/JoeAllen/mqttx";

/// 更新检查/安装在设置对话框内的展示状态。
#[derive(Debug, Clone)]
enum UpdateUi {
    Idle,
    Checking,
    UpToDate,
    Available(Box<UpdateInfo>),
    Downloading { downloaded: u64, total: u64 },
    /// 已下载到暂存区，等待用户选择安装时机
    ReadyToInstall(Box<update::StagedUpdate>),
    Failed(String),
}

struct SettingsDialog {
    engine: Arc<MqttEngine>,
    theme: Entity<SelectState<OptionDelegate>>,
    max_messages: Entity<InputState>,
    show_millis: bool,
    auto_check_update: bool,
    data_dir: PathBuf,
    log_dir: PathBuf,
    update: UpdateUi,
}

impl SettingsDialog {
    fn new(
        engine: Arc<MqttEngine>,
        initial: AppSettings,
        data_dir: PathBuf,
        log_dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let theme_idx = match initial.theme {
            ThemeModePref::System => 0,
            ThemeModePref::Light => 1,
            ThemeModePref::Dark => 2,
        };
        Self {
            engine,
            theme: make_select(&THEMES, theme_idx, window, cx),
            max_messages: cx.new(|cx| {
                InputState::new(window, cx).default_value(initial.max_messages.to_string())
            }),
            show_millis: initial.show_millis,
            auto_check_update: initial.auto_check_update,
            data_dir,
            log_dir,
            update: UpdateUi::Idle,
        }
    }

    fn run_check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.update = UpdateUi::Checking;
        cx.notify();
        let rx = self.engine.run_blocking(update::check_latest);
        cx.spawn_in(window, async move |this, cx| {
            let result = rx.recv().await.unwrap_or(Err("任务丢失".into()));
            this.update_in(cx, |s, _window, cx| {
                s.update = match result {
                    Ok(Some(info)) => UpdateUi::Available(Box::new(info)),
                    Ok(None) => UpdateUi::UpToDate,
                    Err(e) => UpdateUi::Failed(e),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn run_update(&mut self, info: &UpdateInfo, window: &mut Window, cx: &mut Context<Self>) {
        self.update = UpdateUi::Downloading { downloaded: 0, total: info.size };
        cx.notify();
        let (prog_tx, prog_rx) = smol::channel::unbounded();
        let info = info.clone();
        let rx: smol::channel::Receiver<Result<update::StagedUpdate, String>> =
            self.engine.run_blocking(move || {
                update::download_and_stage(&info, &|d, t| {
                    let _ = prog_tx.try_send((d, t));
                })
            });
        cx.spawn_in(window, async move |this, cx| {
            while let Ok((downloaded, total)) = prog_rx.recv().await {
                this.update_in(cx, |s, _window, cx| {
                    s.update = UpdateUi::Downloading { downloaded, total };
                    cx.notify();
                })
                .ok();
            }
            // 进度通道关闭：拿到最终结果
            let result = rx.recv().await.unwrap_or(Err("任务丢失".into()));
            this.update_in(cx, |s, _window, cx| {
                s.update = match result {
                    Ok(staged) => UpdateUi::ReadyToInstall(Box::new(staged)),
                    Err(e) => UpdateUi::Failed(e),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn fmt_mb(bytes: u64) -> String {
        format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
    }

    /// 关于区的更新行：按钮 + 状态 + 下载进度。
    fn render_update_row(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let row = |button: gpui_kit::AnyElement, status: gpui_kit::AnyElement, accent: bool| {
            h_flex()
                .gap_2()
                .items_center()
                .child(button)
                .child(
                    div()
                        .text_xs()
                        .text_color(if accent {
                            cx.theme().accent
                        } else {
                            cx.theme().muted_foreground
                        })
                        .child(status),
                )
        };
        match &self.update {
            UpdateUi::Idle => row(
                Button::new("check-update")
                    .label("检查更新")
                    .outline()
                    .xsmall()
                    .on_click(cx.listener(|this, _, window, cx| this.run_check(window, cx)))
                    .into_any_element(),
                "检查 Gitea Release 上的新版本".into_any_element(),
                false,
            ),
            UpdateUi::Checking => row(
                Button::new("check-update")
                    .label("检查中…")
                    .outline()
                    .xsmall()
                    .loading(true)
                    .into_any_element(),
                "正在检查更新…".into_any_element(),
                false,
            ),
            UpdateUi::UpToDate => row(
                Button::new("check-update")
                    .label("重新检查")
                    .outline()
                    .xsmall()
                    .on_click(cx.listener(|this, _, window, cx| this.run_check(window, cx)))
                    .into_any_element(),
                "已是最新版本".into_any_element(),
                false,
            ),
            UpdateUi::Available(info) => {
                let ver = info.version.clone();
                let info_for_click = info.clone();
                row(
                    Button::new("apply-update")
                        .label(format!("立即更新到 v{}", ver))
                        .primary()
                        .xsmall()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.run_update(&info_for_click, window, cx)
                        }))
                        .into_any_element(),
                    format!("发现新版本 v{}，下载后自动替换重启", ver).into_any_element(),
                    true,
                )
            }
            UpdateUi::ReadyToInstall(staged) => {
                let ver = staged.version.clone();
                let staged_now = staged.clone();
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().accent)
                            .child(format!(
                                "新版本 v{} 已下载并通过校验，等待安装",
                                ver
                            )),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(
                                Button::new("upd-now")
                                    .label("立即安装")
                                    .primary()
                                    .xsmall()
                                    .on_click(move |_, window, cx| {
                                        if let Err(e) = update::install_staged(&staged_now) {
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
                            )
                            .child(
                                Button::new("upd-later")
                                    .label("下次启动安装")
                                    .outline()
                                    .xsmall()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        // 写入同意标记：下次启动据此自动安装
                                        if let UpdateUi::ReadyToInstall(staged) = &this.update {
                                            // 同意标记绑定版本 + 暂存文件哈希
                                            update::mark_install_consent(staged);
                                        }
                                        this.update = UpdateUi::Idle;
                                        window.push_notification(
                                            Notification::info(
                                                "已暂存，下次启动时将自动安装",
                                            ),
                                            cx,
                                        );
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("upd-discard")
                                    .label("放弃此次更新")
                                    .outline()
                                    .xsmall()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let UpdateUi::ReadyToInstall(staged) = &this.update {
                                            update::discard_staged(staged);
                                        }
                                        this.update = UpdateUi::Idle;
                                        window.push_notification(
                                            Notification::info("已删除暂存的更新包"),
                                            cx,
                                        );
                                        cx.notify();
                                    })),
                            ),
                    )
            }
            UpdateUi::Downloading { downloaded, total } => {
                let pct = if *total > 0 {
                    *downloaded as f32 / *total as f32
                } else {
                    0.
                };
                let status = if *total > 0 {
                    format!(
                        "下载中 {:.0}%（{} / {}）",
                        pct * 100.,
                        Self::fmt_mb(*downloaded),
                        Self::fmt_mb(*total)
                    )
                } else {
                    format!("下载中 {}", Self::fmt_mb(*downloaded))
                };
                v_flex()
                    .gap_1()
                    .child(
                        gpui_kit::component::progress::Progress::new("update-progress")
                            .value(pct * 100.),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(status),
                    )
            }
            UpdateUi::Failed(e) => row(
                Button::new("check-update")
                    .label("重试")
                    .outline()
                    .xsmall()
                    .on_click(cx.listener(|this, _, window, cx| this.run_check(window, cx)))
                    .into_any_element(),
                format!("更新失败：{e}").into_any_element(),
                false,
            ),
        }
    }


    fn collect(&self, cx: &App) -> Result<AppSettings, String> {
        let theme = match self.theme.read(cx).selected_value() {
            Some(1) => ThemeModePref::Light,
            Some(2) => ThemeModePref::Dark,
            _ => ThemeModePref::System,
        };
        // 显式校验范围而非静默钳制：越界直接报错，不落盘
        let max_messages = self
            .max_messages
            .read(cx)
            .value()
            .trim()
            .parse::<usize>()
            .map_err(|_| format!(
                "消息缓存条数需要是 {MAX_MESSAGES_MIN}~{MAX_MESSAGES_MAX} 的整数"
            ))?;
        if !(MAX_MESSAGES_MIN..=MAX_MESSAGES_MAX).contains(&max_messages) {
            return Err(format!(
                "消息缓存条数需要是 {MAX_MESSAGES_MIN}~{MAX_MESSAGES_MAX} 的整数"
            ));
        }
        // 四个字段全部显式写出，避免 ..Default::default() 把开关项重置为默认值
        Ok(AppSettings {
            theme,
            max_messages,
            show_millis: self.show_millis,
            auto_check_update: self.auto_check_update,
        })
    }
}

/// 用系统文件管理器打开目录；失败弹 toast（不写日志，避免与日志落盘互相触发）。
fn open_in_file_manager(path: &Path, window: &mut Window, cx: &mut App) {
    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("explorer");
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");

    cmd.arg(path);
    if let Err(e) = cmd.spawn() {
        window.push_notification(Notification::error(format!("打开目录失败: {e}")), cx);
    }
}

impl Render for SettingsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let data_dir = self.data_dir.to_string_lossy().into_owned();
        let log_dir = self.log_dir.to_string_lossy().into_owned();
        let data_dir_for_open = self.data_dir.clone();
        let log_dir_for_open = self.log_dir.clone();

        v_flex()
            .gap_4()
            .w(px(500.))
            .child(field("主题", Select::new(&self.theme)))
            .child(field(
                "每条连接内存中保留的消息条数（100~100000）",
                Input::new(&self.max_messages),
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "另有每连接 {}MB 的字节预算兜底：大报文流会优先按字节从最旧开始驱逐",
                        MAX_CONNECTION_MESSAGE_BYTES / (1024 * 1024)
                    )),
            )
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_sm().child("时间戳显示毫秒"))
                    .child(
                        Switch::new("show-millis")
                            .checked(self.show_millis)
                            .on_change(cx.listener(|this, v, _, cx| {
                                this.show_millis = *v;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_sm().child("自动检查更新（启动时）"))
                    .child(
                        Switch::new("auto-check-update")
                            .checked(self.auto_check_update)
                            .on_change(cx.listener(|this, v, _, cx| {
                                this.auto_check_update = *v;
                                cx.notify();
                            })),
                    ),
            )
            .child(div().h(px(1.)).bg(cx.theme().border).w_full())
            .child(field(
                "数据目录",
                h_flex()
                    .gap_1()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_xs()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(cx.theme().muted_foreground)
                            .child(data_dir.clone()),
                    )
                    .child(Clipboard::new("copy-data-dir").value(data_dir.clone()))
                    .child(
                        Button::new("open-data-dir")
                            .icon(IconName::FolderOpen)
                            .label("打开目录")
                            .outline()
                            .xsmall()
                            .on_click(move |_, window, cx| {
                                open_in_file_manager(&data_dir_for_open, window, cx);
                            }),
                    ),
            ))
            .child(field(
                "日志目录",
                h_flex()
                    .gap_1()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_xs()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(cx.theme().muted_foreground)
                            .child(log_dir.clone()),
                    )
                    .child(
                        Button::new("open-log-dir")
                            .icon(IconName::FolderOpen)
                            .label("打开日志目录")
                            .outline()
                            .xsmall()
                            .on_click(move |_, window, cx| {
                                // 日志目录可能尚未创建（从未产生过日志）
                                if let Err(e) = std::fs::create_dir_all(&log_dir_for_open) {
                                    window.push_notification(
                                        Notification::error(format!("创建日志目录失败: {e}")),
                                        cx,
                                    );
                                    return;
                                }
                                open_in_file_manager(&log_dir_for_open, window, cx);
                            }),
                    ),
            ))
            .child(div().h(px(1.)).bg(cx.theme().border).w_full())
            .child(
                v_flex().gap_1().child(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().font_semibold().child("关于"))
                        .child(
                            h_flex()
                                .gap_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "版本 {}（更新源 {}）",
                                    update::current_version(),
                                    update::UPDATE_SOURCE
                                ))
                                .child("Apache-2.0 许可证"),
                        )
                        .child(self.render_update_row(window, cx))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("对标 MQTTX 的 GPUI 跨平台 MQTT 调试客户端"),
                        )
                        .child(
                            h_flex().gap_1().child(
                                Button::new("about-site")
                                    .label("mqttx.app")
                                    .ghost()
                                    .xsmall()
                                    .tooltip(SITE_URL)
                                    .on_click(|_, _, cx| cx.open_url(SITE_URL)),
                            ).child(
                                Button::new("about-repo")
                                    .label("gitea.heavenlybook.cn/JoeAllen/mqttx")
                                    .ghost()
                                    .xsmall()
                                    .tooltip(REPO_URL)
                                    .on_click(|_, _, cx| cx.open_url(REPO_URL)),
                            ),
                        ),
                ),
            )
    }
}

pub fn open(app: Entity<MqttXApp>, window: &mut Window, cx: &mut App) {
    // 对话框是栈式叠加：标题栏菜单、快捷键、行内按钮等多个入口都会调 open，
    // 不加守卫会压入第二个模态，同 id 的控件在两层之间串台
    if window.has_active_dialog(cx) {
        return;
    }
    let (engine, initial, data_dir, log_dir) = {
        let state = app.read(cx);
        (
            state.engine.clone(),
            state.settings.clone(),
            state.data_dir(),
            state.log_dir(),
        )
    };
    let dialog_view: Entity<SettingsDialog> = cx.new(|cx| {
        SettingsDialog::new(engine, initial, data_dir, log_dir, window, cx)
    });
    let app_save = app.clone();
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let body = dialog_view.clone();
        let for_collect = dialog_view.clone();
        let app_for_save = app_save.clone();
        dialog
            .w(px(560.))
            .title("设置")
            .child(body)
            .footer(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .w_full()
                    .child(
                        Button::new("settings-cancel")
                            .label("取消")
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("settings-ok")
                            .label("保存")
                            .primary()
                            .on_click(move |_, window, cx| {
                                let result = for_collect.read(cx).collect(cx);
                                match result {
                                    Ok(settings) => {
                                        app_for_save.update(cx, |a, cx| {
                                            a.save_settings(settings, cx);
                                            cx.notify();
                                        });
                                        window.close_dialog(cx);
                                    }
                                    Err(e) => {
                                        window.push_notification(Notification::error(e), cx);
                                    }
                                }
                            }),
                    ),
            )
    });
}
