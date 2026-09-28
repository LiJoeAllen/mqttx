//! 应用设置对话框：主题、消息缓存、时间戳、自动检查更新、数据/日志目录、关于。

use std::path::{Path, PathBuf};

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

use crate::model::{AppSettings, ThemeModePref};
use crate::ui::app::MqttXApp;
use crate::ui::widgets::{field, make_select, OptionDelegate};
use crate::ui::IconName;

const THEMES: [&str; 3] = ["跟随系统", "浅色", "深色"];

const SITE_URL: &str = "https://mqttx.app";
const REPO_URL: &str = "https://gitea.heavenlybook.cn/JoeAllen/mqttx";

struct SettingsDialog {
    theme: Entity<SelectState<OptionDelegate>>,
    max_messages: Entity<InputState>,
    show_millis: bool,
    auto_check_update: bool,
    data_dir: PathBuf,
    log_dir: PathBuf,
}

impl SettingsDialog {
    fn new(
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
            theme: make_select(&THEMES, theme_idx, window, cx),
            max_messages: cx.new(|cx| {
                InputState::new(window, cx).default_value(initial.max_messages.to_string())
            }),
            show_millis: initial.show_millis,
            auto_check_update: initial.auto_check_update,
            data_dir,
            log_dir,
        }
    }

    fn collect(&self, cx: &App) -> Result<AppSettings, String> {
        let theme = match self.theme.read(cx).selected_value() {
            Some(1) => ThemeModePref::Light,
            Some(2) => ThemeModePref::Dark,
            _ => ThemeModePref::System,
        };
        let max_messages = self
            .max_messages
            .read(cx)
            .value()
            .trim()
            .parse::<usize>()
            .map_err(|_| "消息缓存条数必须是正整数".to_string())?
            .max(100);
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let data_dir = self.data_dir.to_string_lossy().into_owned();
        let log_dir = self.log_dir.to_string_lossy().into_owned();
        let data_dir_for_open = self.data_dir.clone();
        let log_dir_for_open = self.log_dir.clone();

        v_flex()
            .gap_4()
            .w(px(500.))
            .child(field("主题", Select::new(&self.theme)))
            .child(field(
                "每条连接内存中保留的消息条数",
                Input::new(&self.max_messages),
            ))
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
                    .child(div().text_sm().child("自动检查更新"))
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
                                .child(format!("版本 {}", env!("CARGO_PKG_VERSION")))
                                .child("Apache-2.0 许可证"),
                        )
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
    let (initial, data_dir, log_dir) = {
        let state = app.read(cx);
        (state.settings.clone(), state.data_dir(), state.log_dir())
    };
    let dialog_view: Entity<SettingsDialog> = cx.new(|cx| {
        SettingsDialog::new(initial, data_dir, log_dir, window, cx)
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
