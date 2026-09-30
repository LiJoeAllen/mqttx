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
use crate::model::{render_will_templates, ConnectionConfig};
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
    product_key: Entity<InputState>,
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
        // 表单已用 first 预填：selected 也指向它，否则"打开即保存"会用相同的
        // 内容 push 出一条重复预设（selected=None 走新建分支、另生成 uuid）
        let selected = (!presets.is_empty()).then_some(0);
        Self {
            app,
            presets,
            selected,
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
            product_key: text_field!(cx, window, &first.product_key, "一机一密：ProductKey"),
            device_secret: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(&first.device_secret)
                    .placeholder("一机一密：DeviceSecret")
                    .masked(true)
            }),
        }
    }

    /// 选中项回填表单。此前只做高亮，保存/连接读的是表单旧值，
    /// 会把所选预设用另一条的数据覆盖，或用错误凭据生成连接。
    fn select_preset(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.presets.get(idx).cloned() else {
            return;
        };
        self.selected = Some(idx);
        self.name.update(cx, |s, cx| s.set_value(p.name, window, cx));
        self.group.update(cx, |s, cx| s.set_value(p.group, window, cx));
        self.instance_id
            .update(cx, |s, cx| s.set_value(p.instance_id, window, cx));
        self.region
            .update(cx, |s, cx| s.set_value(p.region, window, cx));
        let mode_idx = match p.auth_mode {
            AliyunAuthMode::Token => 0,
            AliyunAuthMode::DeviceCredential => 1,
        };
        self.auth_mode.update(cx, |s, cx| {
            s.set_selected_index(
                Some(gpui_kit::component::IndexPath::new(mode_idx)),
                window,
                cx,
            )
        });
        self.access_key_id
            .update(cx, |s, cx| s.set_value(p.access_key_id, window, cx));
        self.access_key_secret
            .update(cx, |s, cx| s.set_value(p.access_key_secret, window, cx));
        self.group_id
            .update(cx, |s, cx| s.set_value(p.group_id, window, cx));
        self.device_id
            .update(cx, |s, cx| s.set_value(p.device_id, window, cx));
        self.product_key
            .update(cx, |s, cx| s.set_value(p.product_key, window, cx));
        self.device_secret
            .update(cx, |s, cx| s.set_value(p.device_secret, window, cx));
        cx.notify();
    }

    /// 清空表单进入「新建」工作流。
    fn new_preset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let p = AliyunPreset::new();
        self.selected = None;
        self.name.update(cx, |s, cx| s.set_value(p.name, window, cx));
        self.group.update(cx, |s, cx| s.set_value("", window, cx));
        self.instance_id
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.region.update(cx, |s, cx| s.set_value("", window, cx));
        self.auth_mode.update(cx, |s, cx| {
            s.set_selected_index(Some(gpui_kit::component::IndexPath::new(0)), window, cx)
        });
        self.access_key_id
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.access_key_secret
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.group_id.update(cx, |s, cx| s.set_value("", window, cx));
        self.device_id
            .update(cx, |s, cx| s.set_value(p.device_id, window, cx));
        self.product_key
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.device_secret
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
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
            product_key: v(&self.product_key),
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
                    // 遗嘱字段同样走 {{变量}} 注入（与其它连接入口保持一致）
                    render_will_templates(&mut conn, &a.variables);
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
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_preset(i, window, cx);
                                    }))
                            }),
                    )
                    .child(
                        Button::new("aliyun-new")
                            .icon(IconName::Plus)
                            .label("新建")
                            .ghost()
                            .small()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.new_preset(window, cx);
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
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div().flex_1().min_w(px(0.)).child(field(
                                    "ProductKey（一机一密）",
                                    Input::new(&self.product_key),
                                )),
                            )
                            .child(
                                div().flex_1().min_w(px(0.)).child(field(
                                    "DeviceSecret（一机一密）",
                                    Input::new(&self.device_secret).mask_toggle(),
                                )),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Token 模式（云消息队列 MQTT 版）：接入点 {实例ID}.mqtt.aliyuncs.com，clientId={GroupId}@@@{设备ID}，密码为 HMAC-SHA1 签名；一机一密（物联网平台）：接入点 {ProductKey}.iot-as-mqtt.{地域}.aliyuncs.com，按官方三元组规范签名"),
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
    // 对话框是栈式叠加：标题栏菜单、快捷键、行内按钮等多个入口都会调 open，
    // 不加守卫会压入第二个模态，同 id 的控件在两层之间串台
    if window.has_active_dialog(cx) {
        return;
    }
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
