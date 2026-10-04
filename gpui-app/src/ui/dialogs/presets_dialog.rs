//! 发布预设管理对话框：列出全部 `PublishPreset`，支持新建 / 选中编辑 / 删除（二次确认）。
//! 保存按 **id** 更新，name 与其他预设冲突时报错不落盘；模板存原文，发送时才渲染。

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    h_flex, notification::Notification, v_flex, ActiveTheme as _, Disableable as _,
    Sizable as _, StyledExt as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use crate::ui::IconName;
use crate::ui::i18n;
use gpui_kit::{
    div, px, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window,
};

use crate::model::{PayloadFormat, PublishParams, PublishPreset};
use crate::ui::app::MqttXApp;
use crate::ui::widgets::{field, make_select, KvEditor, OptionDelegate};

const QOS: [&str; 3] = ["QoS 0", "QoS 1", "QoS 2"];
const PAYLOAD_FORMATS: [&str; 4] = ["Plaintext", "JSON", "Base64", "Hex"];
/// 列表里 payload 预览的最大字符数（压平换行后截断）
const LIST_PAYLOAD_PREVIEW_CHARS: usize = 24;

struct PresetsDialog {
    app: Entity<MqttXApp>,
    /// 与 `app.presets` 同步的本地工作副本；所有变更经 `persist` 落盘
    presets: Vec<PublishPreset>,
    /// 当前编辑项下标；None = 未选中（保存/删除禁用）
    selected: Option<usize>,

    name: Entity<InputState>,
    topic: Entity<InputState>,
    payload: Entity<TextareaState>,
    qos: Entity<SelectState<OptionDelegate>>,
    retain: bool,
    payload_format: Entity<SelectState<OptionDelegate>>,
    content_type: Entity<InputState>,
    msg_expiry: Entity<InputState>,
    response_topic: Entity<InputState>,
    correlation_data: Entity<InputState>,
    user_props: Entity<KvEditor>,
}

/// 列表 payload 预览：换行/制表符压平后截取前 N 字符。
fn payload_preview(payload: &str) -> String {
    let flat: String = payload
        .chars()
        .map(|c| match c {
            '\n' | '\r' | '\t' => ' ',
            _ => c,
        })
        .collect();
    let mut out: String = flat.chars().take(LIST_PAYLOAD_PREVIEW_CHARS).collect();
    if flat.chars().count() > LIST_PAYLOAD_PREVIEW_CHARS {
        out.push('…');
    }
    out
}

impl PresetsDialog {
    fn new(
        app: Entity<MqttXApp>,
        presets: Vec<PublishPreset>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // 有预设时默认选中第一项并回填表单，避免打开即空白
        let first = presets.first().cloned();
        let selected = if presets.is_empty() { None } else { Some(0) };
        let params = first
            .as_ref()
            .map(|p| p.params.clone())
            .unwrap_or_default();

        let name_value = first.as_ref().map(|p| p.name.clone()).unwrap_or_default();
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(i18n::t("pub.preset_name_hint"))
                .default_value(name_value)
        });
        let topic = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(i18n::t("pub.topic_preset_ph"))
                .default_value(params.topic.clone())
        });
        let payload = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(i18n::t("pub.payload_preset_ph"))
                .default_value(params.payload.clone())
        });
        let qos = make_select(&QOS, params.qos.min(2) as usize, window, cx);
        let fmt_idx = PayloadFormat::ALL
            .iter()
            .position(|f| *f == params.payload_format)
            .unwrap_or(0);
        let payload_format = make_select(&PAYLOAD_FORMATS, fmt_idx, window, cx);
        let content_type = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(i18n::t("pub.content_type_ph"))
                .default_value(params.content_type.clone().unwrap_or_default())
        });
        let msg_expiry = cx.new(|cx| {
            InputState::new(window, cx).placeholder(i18n::t("pub.seconds_hint")).default_value(
                params
                    .message_expiry_interval
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            )
        });
        let response_topic = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(i18n::t("pub.response_topic_ph"))
                .default_value(params.response_topic.clone().unwrap_or_default())
        });
        let correlation_data = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(i18n::t("pub.correlation_ph"))
                .default_value(params.correlation_data.clone().unwrap_or_default())
        });
        let user_props =
            cx.new(|cx| KvEditor::new("preset-props", &params.user_properties, window, cx));

        Self {
            app,
            presets,
            selected,
            name,
            topic,
            payload,
            qos,
            retain: params.retain,
            payload_format,
            content_type,
            msg_expiry,
            response_topic,
            correlation_data,
            user_props,
        }
    }

    /// 把本地工作副本写回 `app.presets` 并落盘（内部调 `save_presets`）。
    fn persist(&self, cx: &mut Context<Self>) {
        let presets = self.presets.clone();
        self.app.update(cx, |a, cx| {
            a.save_presets(presets);
            cx.notify();
        });
    }

    /// 读取表单当前值为发布参数（不渲染模板，存原文）。
    fn collect(&self, cx: &App) -> PublishParams {
        let qos = self
            .qos
            .read(cx)
            .selected_value()
            .copied()
            .unwrap_or(0) as u8;
        let format = PayloadFormat::ALL
            [self.payload_format.read(cx).selected_value().copied().unwrap_or(0)];
        let opt = |s: String| {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        PublishParams {
            topic: self.topic.read(cx).value().to_string(),
            payload: self.payload.read(cx).value().to_string(),
            payload_format: format,
            qos,
            retain: self.retain,
            user_properties: self.user_props.read(cx).pairs(cx),
            content_type: opt(self.content_type.read(cx).value().to_string()),
            message_expiry_interval: {
                let t = self.msg_expiry.read(cx).value().to_string();
                let t = t.trim();
                if t.is_empty() {
                    None
                } else {
                    t.parse::<u32>().ok()
                }
            },
            response_topic: opt(self.response_topic.read(cx).value().to_string()),
            correlation_data: opt(self.correlation_data.read(cx).value().to_string()),
            ..Default::default()
        }
    }

    /// 选中项回填表单，进入编辑态。
    fn load(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.presets.get(idx).cloned() else {
            return;
        };
        self.selected = Some(idx);
        let params = p.params.clone();
        self.name
            .update(cx, |s, cx| s.set_value(p.name, window, cx));
        self.topic
            .update(cx, |s, cx| s.set_value(params.topic.clone(), window, cx));
        self.payload
            .update(cx, |s, cx| s.set_value(params.payload.clone(), window, cx));
        let qos_idx = params.qos.min(2) as usize;
        self.qos.update(cx, |s, cx| {
            s.set_selected_index(Some(gpui_kit::component::IndexPath::new(qos_idx)), window, cx)
        });
        let fmt_idx = PayloadFormat::ALL
            .iter()
            .position(|f| *f == params.payload_format)
            .unwrap_or(0);
        self.payload_format.update(cx, |s, cx| {
            s.set_selected_index(
                Some(gpui_kit::component::IndexPath::new(fmt_idx)),
                window,
                cx,
            )
        });
        self.retain = params.retain;
        let ct = params.content_type.clone().unwrap_or_default();
        self.content_type
            .update(cx, |s, cx| s.set_value(ct, window, cx));
        let expiry = params
            .message_expiry_interval
            .map(|v| v.to_string())
            .unwrap_or_default();
        self.msg_expiry
            .update(cx, |s, cx| s.set_value(expiry, window, cx));
        let rt = params.response_topic.clone().unwrap_or_default();
        self.response_topic
            .update(cx, |s, cx| s.set_value(rt, window, cx));
        let cd = params.correlation_data.clone().unwrap_or_default();
        self.correlation_data
            .update(cx, |s, cx| s.set_value(cd, window, cx));
        // KvEditor 不支持整体回填，直接重建实体
        self.user_props =
            cx.new(|cx| KvEditor::new("preset-props", &params.user_properties, window, cx));
        cx.notify();
    }

    /// 清空表单并退出编辑态（删除后无剩余预设时调用）。
    fn clear_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = None;
        self.name.update(cx, |s, cx| s.set_value("", window, cx));
        self.topic.update(cx, |s, cx| s.set_value("", window, cx));
        self.payload
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.qos.update(cx, |s, cx| {
            s.set_selected_index(Some(gpui_kit::component::IndexPath::new(0)), window, cx)
        });
        self.payload_format.update(cx, |s, cx| {
            s.set_selected_index(Some(gpui_kit::component::IndexPath::new(0)), window, cx)
        });
        self.retain = false;
        self.content_type
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.msg_expiry
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.response_topic
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.correlation_data
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.user_props = cx.new(|cx| KvEditor::new("preset-props", &[], window, cx));
        cx.notify();
    }

    /// 「新建」：创建默认名称预设（重名自动加序号）并立即落盘、进入编辑。
    fn create_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut name = i18n::t("preset.default_name").to_string();
        let mut n = 2;
        while self.presets.iter().any(|p| p.name == name) {
            name = i18n::tf("preset.default_name_n", &[("n", &n)]);
            n += 1;
        }
        self.presets.push(PublishPreset::new(name, PublishParams::default()));
        let idx = self.presets.len() - 1;
        self.persist(cx);
        self.load(idx, window, cx);
    }

    /// 「保存」：name 校验 + 冲突检查通过后按 id 更新，再落盘。
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(idx) = self.selected else {
            window.push_notification(Notification::warning(i18n::t("preset.pick_first")), cx);
            return;
        };
        let name = self.name.read(cx).value().trim().to_string();
        if name.is_empty() {
            window.push_notification(Notification::warning(i18n::t("pub.preset_name_required")), cx);
            return;
        }
        // 与其他预设（不含自身）重名 → 报错且不落盘
        let own_id = self.presets[idx].id.clone();
        if self
            .presets
            .iter()
            .any(|p| p.id != own_id && p.name == name)
        {
            window.push_notification(
                Notification::error(i18n::tf(
                    "preset.name_exists",
                    &[("name", &name)],
                )),
                cx,
            );
            return;
        }
        // 过期秒数输入了但解析失败 → 拒绝保存（与发布路径校验一致）
        let expiry_raw = self.msg_expiry.read(cx).value().to_string();
        let params = self.collect(cx);
        if !expiry_raw.trim().is_empty() && params.message_expiry_interval.is_none() {
            window.push_notification(
                Notification::warning(i18n::t("pub.expiry_invalid")),
                cx,
            );
            return;
        }
        // 按 id 更新：id 不随改名变化，避免并发列表错位
        if let Some(p) = self.presets.iter_mut().find(|p| p.id == own_id) {
            p.name = name.clone();
            p.params = params;
        }
        self.persist(cx);
        window.push_notification(Notification::success(i18n::tf(
                "preset.saved",
                &[("name", &name)],
            )), cx);
    }

    /// 「删除」：`open_alert_dialog` 二次确认后从列表移除并落盘。
    fn ask_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(idx) = self.selected else {
            window.push_notification(Notification::warning(i18n::t("preset.pick_delete_first")), cx);
            return;
        };
        let pid = self.presets[idx].id.clone();
        let pname = self.presets[idx].name.clone();
        let weak = cx.weak_entity();
        // 与发布预设下拉的删除确认保持一致：不可撤销操作先弹二次确认
        window.open_alert_dialog(cx, move |alert, _, _| {
            let weak = weak.clone();
            let pid = pid.clone();
            alert
                .title(i18n::t("pub.delete_title"))
                .description(i18n::tf(
                    "pub.delete_confirm",
                    &[("name", &pname)],
                ))
                .button_props(
                    DialogButtonProps::default()
                        .show_cancel(true)
                        .cancel_text(i18n::t("common.cancel"))
                        .ok_text(i18n::t("common.delete"))
                        .ok_variant(ButtonVariant::Danger),
                )
                .on_ok(move |_, window, cx| {
                    weak.update(cx, |d, cx| {
                        d.presets.retain(|p| p.id != pid);
                        d.persist(cx);
                        if d.presets.is_empty() {
                            d.clear_form(window, cx);
                        } else {
                            // 删除后选中项前移，取原位置（越界则取最后一项）
                            let next = idx.min(d.presets.len() - 1);
                            d.load(next, window, cx);
                        }
                    })
                    .ok();
                    true
                })
        });
    }
}

impl Render for PresetsDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let selected_bg = cx.theme().secondary;
        let hover_bg = cx.theme().muted;
        let mono_font = cx.theme().mono_font_family.clone();

        // 左：预设列表（name + topic + payload 前 N 字符；空态提示）
        let mut list = v_flex().gap_0p5();
        if self.presets.is_empty() {
            list = list.child(
                v_flex()
                    .p_3()
                    .gap_1()
                    .items_center()
                    .child(
                        gpui_kit::component::Icon::new(IconName::Bookmark)
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .text_center()
                            .child(i18n::t("preset.empty")),
                    ),
            );
        } else {
            for (i, p) in self.presets.iter().enumerate() {
                let selected = self.selected == Some(i);
                let name = p.name.clone();
                let topic = p.params.topic.clone();
                let preview = payload_preview(&p.params.payload);
                list = list.child(
                    div()
                        .id(("preset-list-item", i))
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .when(selected, |d| d.bg(selected_bg))
                        .when(!selected, |d| d.hover(move |this| this.bg(hover_bg)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.load(i, window, cx);
                        }))
                        .child(
                            v_flex()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_sm()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(topic),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_family(mono_font.clone())
                                        .text_color(muted)
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(preview),
                                ),
                        ),
                );
            }
        }

        // 右：编辑表单（不渲染模板，全部存原文）
        let retain = self.retain;
        let form = v_flex()
            .flex_1()
            .min_w(px(0.))
            .gap_2()
            .child(field(i18n::t("aliyun.preset_name"), Input::new(&self.name)))
            .child(field(i18n::t("pub.topic_placeholder"), Input::new(&self.topic)))
            .child(field(
                i18n::t("preset.payload_field"),
                Textarea::new(&self.payload).h(px(96.)),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .w(px(110.))
                            .child(field("QoS", Select::new(&self.qos).small())),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .pt_4()
                            .child(
                                Switch::new("preset-retain")
                                    .checked(retain)
                                    .on_change(cx.listener(|this, v, _, cx| {
                                        this.retain = *v;
                                        cx.notify();
                                    })),
                            )
                            .child(div().text_xs().child("Retain")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(field(
                                i18n::t("preset.payload_format"),
                                Select::new(&self.payload_format).small(),
                            )),
                    ),
            )
            .child(div().h(px(1.)).bg(cx.theme().border).w_full())
            .child(div().text_xs().font_semibold().child(i18n::t("pub.mqtt5_props")))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div().flex_1().min_w(px(0.)).child(field(
                            "Content-Type",
                            Input::new(&self.content_type).small(),
                        )),
                    )
                    .child(
                        div().w(px(120.)).child(field(
                            i18n::t("pub.expiry_field"),
                            Input::new(&self.msg_expiry).small(),
                        )),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div().flex_1().min_w(px(0.)).child(field(
                            "Response Topic",
                            Input::new(&self.response_topic).small(),
                        )),
                    )
                    .child(
                        div().flex_1().min_w(px(0.)).child(field(
                            "Correlation Data",
                            Input::new(&self.correlation_data).small(),
                        )),
                    ),
            )
            .child(field(i18n::t("preset.user_props"), self.user_props.clone()));

        h_flex()
            .gap_3()
            .w_full()
            // 左：列表 + 新建
            .child(
                v_flex()
                    .w(px(220.))
                    .flex_shrink_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(div().text_sm().font_semibold().child(i18n::t("preset.section")))
                            .child(
                                Button::new("preset-new")
                                    .icon(IconName::Plus)
                                    .label(i18n::t("common.new"))
                                    .ghost()
                                    .xsmall()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.create_new(window, cx);
                                    })),
                            ),
                    )
                    .child(list),
            )
            // 右：表单
            .child(form)
    }
}

pub fn open(app: Entity<MqttXApp>, window: &mut Window, cx: &mut App) {
    // 对话框是栈式叠加：标题栏菜单、快捷键、行内按钮等多个入口都会调 open，
    // 不加守卫会压入第二个模态，同 id 的控件在两层之间串台
    if window.has_active_dialog(cx) {
        return;
    }
    let presets = app.read(cx).presets.clone();
    let dialog_view: Entity<PresetsDialog> =
        cx.new(|cx| PresetsDialog::new(app, presets, window, cx));
    window.open_dialog(cx, move |dialog, _, cx| {
        let body = dialog_view.clone();
        let for_save = dialog_view.clone();
        let for_delete = dialog_view.clone();
        // 每次重绘读取选中态，控制保存 / 删除按钮禁用
        let has_selection = dialog_view.read(cx).selected.is_some();
        dialog
            .w(px(760.))
            .title(i18n::t("preset.manage_title"))
            .child(body)
            .footer(
                h_flex()
                    .w_full()
                    .justify_between()
                    .child(
                        Button::new("preset-delete")
                            .icon(IconName::Trash)
                            .label(i18n::t("common.delete"))
                            .danger()
                            .outline()
                            .disabled(!has_selection)
                            .on_click(move |_, window, cx| {
                                for_delete.update(cx, |d, cx| d.ask_delete(window, cx));
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("preset-close")
                                    .label(i18n::t("common.close"))
                                    .outline()
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                Button::new("preset-save")
                                    .label(i18n::t("common.save"))
                                    .primary()
                                    .disabled(!has_selection)
                                    .on_click(move |_, window, cx| {
                                        for_save.update(cx, |d, cx| d.save(window, cx));
                                    }),
                            ),
                    ),
            )
    });
}
