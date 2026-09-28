//! 阿里云 IoT 设备接入对话框：保存预设，并可一键生成连接并连接。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::{
    h_flex, notification::Notification, v_flex, ActiveTheme as _, Sizable as _, StyledExt as _,
    WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use crate::ui::IconName;
use gpui_kit::{
    div, px, App, AppContext as _, Context, Entity, InteractiveElement as _, StatefulInteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, Window,
};

use crate::aliyun::{AliyunAuthMode, AliyunPreset};
use crate::model::ConnectionConfig;
use crate::ui::app::MqttXApp;
use crate::ui::widgets::{field, make_select, OptionDelegate};

const AUTH_MODES: [&str; 2] = ["Group 令牌 (Token)", "一机一密"];

struct AliyunDialog {
    app: Entity<MqttXApp>,
    presets: Vec<AliyunPreset>,
    selected: Option<usize>,

    name: Entity<InputState>,
    group: Entity<InputState>,
    instance_id: Entity<InputState>,
    region: Entity<InputState>,
    auth_mode: Entity<SelectState<OptionDelegate>>,
    access_key_id: Entity<InputState>,
    access_key_secret: Entity<InputState>,
    group_id: Entity<InputState>,
    device_id: Entity<InputState>,
    device_secret: Entity<InputState>,
}

macro_rules! text_field {
    ($cx:expr, $window:expr, $value:expr, $ph:expr) => {
        $cx.new(|cx| {
            InputState::new($window, cx)
                .default_value($value)
                .placeholder($ph)
        })
    };
}

impl AliyunDialog {
    fn new(app: Entity<MqttXApp>, presets: Vec<AliyunPreset>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let first = presets.first().cloned().unwrap_or_else(AliyunPreset::new);
        Self {
            app,
            presets,
            selected: None,
            name: text_field!(cx, window, &first.name, "预设名称"),
            group: text_field!(cx, window, &first.group, "分组（可选）"),
            instance_id: text_field!(cx, window, &first.instance_id, "post-cn-xxxx"),
            region: text_field!(cx, window, &first.region, "如 cn-shanghai（可留空）"),
            auth_mode: make_select(
                &AUTH_MODES,
                match first.auth_mode {
                    AliyunAuthMode::Token => 0,
                    AliyunAuthMode::DeviceCredential => 1,
                },
                window,
                cx,
            ),
            access_key_id: text_field!(cx, window, &first.access_key_id, "AccessKey ID"),
            access_key_secret: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(&first.access_key_secret)
                    .placeholder("AccessKey Secret")
                    .masked(true)
            }),
            group_id: text_field!(cx, window, &first.group_id, "GID-xxx"),
            device_id: text_field!(cx, window, &first.device_id, "设备 ID"),
            device_secret: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(&first.device_secret)
                    .placeholder("一机一密：DeviceSecret")
                    .masked(true)
            }),
        }
    }

    fn collect(&self, cx: &App) -> AliyunPreset {
        let v = |e: &Entity<InputState>| e.read(cx).value().to_string();
        let id = self
            .selected
            .and_then(|i| self.presets.get(i))
            .map(|p| p.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        AliyunPreset {
            id,
            name: v(&self.name),
            group: v(&self.group),
            instance_id: v(&self.instance_id),
            region: v(&self.region),
            auth_mode: match self.auth_mode.read(cx).selected_value() {
                Some(1) => AliyunAuthMode::DeviceCredential,
                _ => AliyunAuthMode::Token,
            },
            access_key_id: v(&self.access_key_id),
            access_key_secret: v(&self.access_key_secret),
            group_id: v(&self.group_id),
            device_id: v(&self.device_id),
            device_secret: v(&self.device_secret),
        }
    }

    fn save_preset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preset = self.collect(cx);
        if preset.name.trim().is_empty() {
            window.push_notification(Notification::warning("请填写预设名称"), cx);
            return;
        }
        match self.selected {
            Some(i) if i < self.presets.len() => self.presets[i] = preset,
            _ => self.presets.push(preset),
        }
        let saved = self.presets.clone();
        self.app.update(cx, |a, cx| {
            a.save_aliyun(saved);
            cx.notify();
        });
        window.push_notification(Notification::success("预设已保存"), cx);
    }

    fn connect(&self, window: &mut Window, cx: &mut Context<Self>) {
        let preset = self.collect(cx);
        match preset.to_connection() {
            Ok(conn) => {
                let mut conn: ConnectionConfig = conn;
                conn.name = preset.name.clone();
                self.app.update(cx, |a, cx| {
                    a.save_connection(conn.clone());
                    a.engine.connect(conn);
                    cx.notify();
                });
                window.close_dialog(cx);
            }
            Err(e) => {
                window.push_notification(Notification::error(format!("无法生成连接: {e}")), cx);
            }
        }
    }
}

impl Render for AliyunDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_3()
            .w_full()
            // 左：预设列表
            .child(
                v_flex()
                    .w(px(160.))
                    .gap_1()
                    .child(div().text_xs().font_semibold().child("已保存预设"))
                    .children(
                        self.presets
                            .iter()
                            .enumerate()
                            .map(|(i, p)| {
                                let selected = self.selected == Some(i);
                                div()
                                    .id(("aliyun-preset", i))
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .text_sm()
                                    .when(selected, |d| d.bg(cx.theme().secondary))
                                    .cursor_pointer()
                                    .child(p.name.clone())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        // 选中仅做高亮；表单编辑的是“新建/另存”工作流
                                        this.selected = Some(i);
                                        cx.notify();
                                    }))
                            }),
                    )
                    .child(
                        Button::new("aliyun-new")
                            .icon(IconName::Plus)
                            .label("新建")
                            .ghost()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selected = None;
                                cx.notify();
                            })),
                    ),
            )
            // 右：表单
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_2()
                    .child(field("预设名称", Input::new(&self.name)))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div().flex_1().min_w(px(0.)).child(field(
                                    "实例 ID",
                                    Input::new(&self.instance_id),
                                )),
                            )
                            .child(
                                div().flex_1().min_w(px(0.)).child(field(
                                    "地域",
                                    Input::new(&self.region),
                                )),
                            ),
                    )
                    .child(field("鉴权方式", Select::new(&self.auth_mode)))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div().flex_1().min_w(px(0.)).child(field(
                                    "AccessKey ID",
                                    Input::new(&self.access_key_id),
                                )),
                            )
                            .child(
                                div().flex_1().min_w(px(0.)).child(field(
                                    "AccessKey Secret",
                                    Input::new(&self.access_key_secret).mask_toggle(),
                                )),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div().flex_1().min_w(px(0.)).child(field(
                                    "Group ID",
                                    Input::new(&self.group_id),
                                )),
                            )
                            .child(
                                div().flex_1().min_w(px(0.)).child(field(
                                    "设备 ID",
                                    Input::new(&self.device_id),
                                )),
                            ),
                    )
                    .child(field(
                        "DeviceSecret（一机一密）",
                        Input::new(&self.device_secret).mask_toggle(),
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Token 模式使用 MQTT 3.1.1，clientId={GroupId}@@@{设备ID}，密码为 HMAC-SHA1 签名"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(
                                Button::new("aliyun-save")
                                    .label("保存预设")
                                    .outline()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_preset(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("aliyun-connect")
                                    .label("生成并连接")
                                    .primary()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.connect(window, cx)
                                    })),
                            ),
                    ),
            )
    }
}

pub fn open(app: Entity<MqttXApp>, window: &mut Window, cx: &mut App) {
    let presets = app.read(cx).aliyun_presets.clone();
    let dialog_view = cx.new(|cx| AliyunDialog::new(app, presets, window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .w(px(760.))
            .title("阿里云 IoT 设备")
            .child(dialog_view.clone())
            .footer(gpui_kit::div())
    });
}
