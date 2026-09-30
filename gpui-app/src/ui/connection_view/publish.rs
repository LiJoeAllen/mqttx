//! 发布：参数收集/发送、发布预设菜单与保存对话框。


use gpui_kit::component::button::{ Button, ButtonVariant };
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{ Input, InputState, Textarea };
use gpui_kit::component::menu::{ PopupMenuItem };
use gpui_kit::component::select::Select;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ h_flex, notification::Notification, v_flex,  };
use crate::ui::IconName;
use gpui_kit::{ div, px, App, Context, Entity, IntoElement, SharedString, Window,  };
use gpui_kit::component::IndexPath;

use crate::model::{ render_template, PayloadFormat, PublishParams,  };
use crate::ui::widgets::{ field, KvEditor };

use super::*;

impl ConnectionView {
    /// 发布栏收起后的细条：抽屉把手。
    pub(super) fn render_publish_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .h(px(34.))
            .px_3()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(
                Button::new("pub-rail-expand")
                    .icon(IconName::PanelBottomOpen)
                    .ghost()
                    .small()
                    .tooltip("展开发布面板")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.publish_collapsed = false;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("发布面板已收起"),
            )
    }

    /// 发布当前面板参数。入口有三：发送按钮、发布主题框内 Enter、
    /// 全局 Ctrl+Enter action（app/mod.rs 注册）。
    pub(crate) fn do_publish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 未连接时消息发不出去，先拦截给出与订阅路径一致的提示
        if !self.engine.is_connected(&self.conn_id) {
            window.push_notification(Notification::warning("未连接：消息未发送"), cx);
            return;
        }
        let mut params = self.collect_publish_params(cx);
        if params.topic.trim().is_empty() {
            window.push_notification(Notification::warning("发布主题不能为空"), cx);
            return;
        }
        // 过期秒数输入非法时 collect 会静默得到 None，这里显式拦截避免误发
        let expiry_raw = self.msg_expiry.read(cx).value().to_string();
        if !expiry_raw.trim().is_empty() && params.message_expiry_interval.is_none() {
            window.push_notification(Notification::warning("消息过期须为非负整数（秒）"), cx);
            return;
        }

        let vars = self.with_app(cx, |app| app.variables.clone()).unwrap_or_default();
        params.topic = render_template(&params.topic, &vars);
        params.payload = render_template(&params.payload, &vars);
        // {{topic}} 等变量渲染后可能变空串，渲染完成后再校验一次
        if params.topic.trim().is_empty() {
            window.push_notification(Notification::warning("发布主题不能为空"), cx);
            return;
        }
        // {{变量}} 同样作用于用户属性的 key/value
        params.user_properties = params
            .user_properties
            .into_iter()
            .map(|(k, v)| (render_template(&k, &vars), render_template(&v, &vars)))
            .collect();
        // v5 文本属性（Content-Type / Response Topic / Correlation Data）一并注入
        params.content_type = params.content_type.map(|v| render_template(&v, &vars));
        params.response_topic = params.response_topic.map(|v| render_template(&v, &vars));
        params.correlation_data = params.correlation_data.map(|v| render_template(&v, &vars));

        if let PayloadFormat::Json = params.payload_format
            && let Err(e) = serde_json::from_str::<serde_json::Value>(&params.payload) {
                window.push_notification(
                    Notification::warning(format!("JSON 格式无效: {e}")),
                    cx,
                );
                return;
            }
        match params.payload_format.encode(&params.payload) {
            Ok(bytes) => params.raw_bytes = Some(bytes),
            Err(e) => {
                window.push_notification(Notification::warning(e), cx);
                return;
            }
        }

        // v3.1.1 忽略 v5 属性，避免残留输入带进发布报文
        if !self.is_v5(cx) {
            params.user_properties.clear();
            params.content_type = None;
            params.message_expiry_interval = None;
            params.response_topic = None;
            params.correlation_data = None;
        }
        // 发出的消息若会被当前过滤条件隐藏（方向/订阅/搜索），发布记录在
        // 消息流里看不到，用户容易误以为没发出去，此时给一条显式反馈
        let query = self.filter.read(cx).value().to_lowercase();
        let hidden = self.msg_dir == DirFilter::Received
            || self
                .sub_filter
                .as_deref()
                .map(|f| !topic_matches(f, &params.topic))
                .unwrap_or(false)
            || (!query.is_empty()
                && !contains_ignore_case(&params.topic, &query)
                && !contains_ignore_case(&params.payload, &query));
        self.engine.publish(self.conn_id.clone(), params);
        if hidden {
            window.push_notification(
                Notification::success("已发送（被当前过滤条件隐藏，消息流中不显示）"),
                cx,
            );
        }
    }

    /// 读取当前发布面板上的参数（模板渲染前的原始值）；保存预设与发布共用，
    /// 须覆盖全部 v5 属性与用户属性。
    fn collect_publish_params(&self, cx: &App) -> PublishParams {
        let qos = self.pub_qos.read(cx).selected_value().copied().unwrap_or(0) as u8;
        let format = PayloadFormat::ALL
            [self.payload_format.read(cx).selected_value().copied().unwrap_or(0)];
        let opt = |s: String| {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        PublishParams {
            topic: self.pub_topic.read(cx).value().to_string(),
            payload: self.payload.read(cx).value().to_string(),
            payload_format: format,
            qos,
            retain: self.retain,
            user_properties: self.user_props.read(cx).pairs(cx),
            content_type: opt(self.content_type.read(cx).value().to_string()),
            message_expiry_interval: {
                let t = self.msg_expiry.read(cx).value().to_string();
                let t = t.trim();
                if t.is_empty() { None } else { t.parse::<u32>().ok() }
            },
            response_topic: opt(self.response_topic.read(cx).value().to_string()),
            correlation_data: opt(self.correlation_data.read(cx).value().to_string()),
            ..Default::default()
        }
    }

    /// 把预设应用到发布面板控件。
    fn apply_publish_preset(
        &mut self,
        params: &PublishParams,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pub_topic
            .update(cx, |s, cx| s.set_value(params.topic.clone(), window, cx));
        self.payload
            .update(cx, |s, cx| s.set_value(params.payload.clone(), window, cx));
        // 预设未指定 Content-Type 时清空旧值，避免上一次输入残留
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
        // KvEditor 不支持运行时整体替换，直接重建实体回填用户属性
        self.user_props = cx.new(|cx| KvEditor::new("pub", &params.user_properties, window, cx));
        let qos_idx = params.qos.min(2) as usize;
        self.pub_qos.update(cx, |s, cx| {
            s.set_selected_index(Some(IndexPath::new(qos_idx)), window, cx)
        });
        let fmt_idx = PayloadFormat::ALL
            .iter()
            .position(|f| *f == params.payload_format)
            .unwrap_or(0);
        self.payload_format.update(cx, |s, cx| {
            s.set_selected_index(Some(IndexPath::new(fmt_idx)), window, cx)
        });
        self.retain = params.retain;
        // 同步模板变量面板：预设替换了模板，旧占位符行必须随之清理，
        // 否则残留行仍会写回全局变量
        self.sync_var_rows(window, cx);
        cx.notify();
    }

pub(super) fn render_publish_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let pub_topic = self.pub_topic.clone();
        let payload = self.payload.clone();
        let pub_qos = self.pub_qos.clone();
        let payload_format = self.payload_format.clone();
        let content_type = self.content_type.clone();
        let msg_expiry = self.msg_expiry.clone();
        let response_topic = self.response_topic.clone();
        let correlation_data = self.correlation_data.clone();
        let user_props = self.user_props.clone();
        let retain = self.retain;
        let show_props = self.show_props;
        let show_vars = self.show_vars;
        let is_v5 = self.is_v5(cx);

        // 发布预设
        let presets = self.with_app(cx, |app| app.presets.clone()).unwrap_or_default();
        let connected = self.engine.is_connected(&self.conn_id);
        // 当前提取到的占位符数量（仅用于提示，行构建在 sync_var_rows 完成）
        let var_count = extract_placeholder_keys(
            &self.pub_topic.read(cx).value(),
            &self.payload.read(cx).value(),
        )
        .len();

        let mut card = v_flex()
            .w_full()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .px_3()
            .py_2()
            .gap_2();

        card = card.child(
            h_flex()
                .gap_2()
                .items_center()
                // 主题框内 Enter 直接发送（Ctrl+Enter 走全局 action，任意焦点可用）
                .child(
                    div()
                        .id(SharedString::from(format!("pub-topic-wrap-{}", self.conn_id)))
                        .flex_1()
                        .min_w(px(0.))
                        .on_key_down(cx.listener(
                            |this, ev: &gpui_kit::KeyDownEvent, window, cx| {
                                if ev.keystroke.key == "enter"
                                    && !ev.keystroke.modifiers.control
                                {
                                    cx.stop_propagation();
                                    this.do_publish(window, cx);
                                }
                            },
                        ))
                        .child(Input::new(&pub_topic).small()),
                )
                .child(div().w(px(92.)).child(Select::new(&pub_qos).small()))
                .child(div().w(px(124.)).child(Select::new(&payload_format).small()))
                .child(self.render_preset_menu(presets, cx))
                .child({
                    // 变量注入开关：展开/收起内联绑定面板
                    let count = var_count;
                    Button::new("vars-toggle")
                        .icon(IconName::Variable)
                        .ghost()
                        .small()
                        .when(show_vars, |b| b.selected(true))
                        .tooltip(if count > 0 {
                            format!("变量注入（{count} 个占位符）")
                        } else {
                            "变量注入".to_string()
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.show_vars {
                                this.show_vars = false;
                                cx.notify();
                            } else {
                                this.sync_var_rows(window, cx);
                            }
                        }))
                })
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(Switch::new("retain").checked(retain).on_change(
                            cx.listener(|this, v, _, cx| {
                                this.retain = *v;
                                cx.notify();
                            }),
                        ))
                        .child(div().text_xs().child("Retain")),
                )
                .when(is_v5, |h| {
                    h.child(
                        Button::new("toggle-props")
                            .icon(IconName::SlidersHorizontal)
                            .ghost()
                            .small()
                            .when(show_props, |b| b.selected(true))
                            .tooltip("MQTT 5 属性")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_props = !this.show_props;
                                cx.notify();
                            })),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!("publish-{}", self.conn_id)))
                        .icon(IconName::SendHorizontal)
                        .label("发送")
                        .primary()
                        .small()
                        // 未连接时禁用，Ctrl+Enter 路径由 do_publish 内的前置检查兜底
                        .disabled(!connected)
                        .when(!connected, |b| b.tooltip("未连接，无法发送"))
                        .on_click(cx.listener(|this, _, window, cx| this.do_publish(window, cx))),
                )
                // 发布栏抽屉把手：收起整个底部面板
                .child(
                    Button::new("pub-collapse")
                        .icon(IconName::PanelBottom)
                        .ghost()
                        .small()
                        .tooltip("收起发布面板")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.publish_collapsed = true;
                            cx.notify();
                        })),
                ),
        );

        card = card.child(
            div()
                .id(SharedString::from(format!("payload-wrap-{}", self.conn_id)))
                .h(px(110.))
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .p_1()
                // Ctrl+Enter 发送由全局 action 兜底（焦点在本框内也能触发）
                .child(Textarea::new(&payload)),
        );

        // 内联变量绑定面板：仅在展开且已提取到占位符时渲染
        if show_vars && !self.var_rows.is_empty() {
            card = card.child(self.render_vars_panel(cx));
        }

        if show_props && is_v5 {
            card = card.child(
                v_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        h_flex()
                            .gap_2()
                            .w_full()
                            .child(
                                div().w(px(220.)).child(field(
                                    "Content-Type",
                                    Input::new(&content_type).small(),
                                )),
                            )
                            .child(
                                div().w(px(120.)).child(field(
                                    "消息过期(秒)",
                                    Input::new(&msg_expiry).small(),
                                )),
                            )
                            .child(
                                div().flex_1().min_w(px(0.)).child(field(
                                    "Response Topic",
                                    Input::new(&response_topic).small(),
                                )),
                            )
                            .child(
                                div().flex_1().min_w(px(0.)).child(field(
                                    "Correlation Data",
                                    Input::new(&correlation_data).small(),
                                )),
                            ),
                    )
                    .child(user_props.clone()),
            );
        }
        card
    }

    /// 发布预设下拉：应用已有预设、保存当前、删除预设。
    fn render_preset_menu(        &self,
        presets: Vec<crate::model::PublishPreset>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let weak_view = cx.weak_entity();
        let preset_name = self.preset_name.clone();
        let app_weak = self.app.clone();
        let has = !presets.is_empty();
        Button::new(SharedString::from(format!("preset-menu-{}", self.conn_id)))
            .icon(IconName::Bookmark)
            .ghost()
            .small()
            .tooltip("消息预设")
            .dropdown_menu(move |mut menu, window, cx| {
                // 应用预设
                for p in &presets {
                    let weak = weak_view.clone();
                    let params = p.params.clone();
                    menu = menu.item(
                        PopupMenuItem::new(SharedString::from(p.name.clone())).on_click(
                            move |_ev, window, cx| {
                                if let Some(view) = weak.upgrade() {
                                    view.update(cx, |view, cx| {
                                        view.apply_publish_preset(&params, window, cx);
                                    });
                                }
                            },
                        ),
                    );
                }
                if has {
                    menu = menu.separator();
                }
                // 保存当前
                let weak_save = weak_view.clone();
                let name_input = preset_name.clone();
                menu = menu.item(PopupMenuItem::new("保存当前为预设…").on_click(
                    move |_ev, window, cx| {
                        if let Some(view) = weak_save.upgrade() {
                            view.update(cx, |view, cx| {
                                view.open_save_preset_dialog(window, cx, name_input.clone());
                            });
                        }
                    },
                ));
                // 打开预设管理对话框（新建 / 编辑 / 删除）
                let manage_app = app_weak.clone();
                menu = menu.item(PopupMenuItem::new("管理预设…").on_click(
                    move |_ev, window, cx| {
                        if let Some(app) = manage_app.upgrade() {
                            crate::ui::dialogs::presets_dialog::open(app, window, cx);
                        }
                    },
                ));
                // 删除子菜单
                if has {
                    let weak_del = weak_view.clone();
                    let del_presets = presets.clone();
                    menu = menu.submenu(
                        "删除预设",
                        window,
                        cx,
                        move |submenu, _window, _cx| {
                            let mut submenu = submenu;
                            for p in &del_presets {
                                let weak2 = weak_del.clone();
                                let id = p.id.clone();
                                let name = p.name.clone();
                                let label = format!("删除「{}」", p.name);
                                submenu = submenu.item(
                                    PopupMenuItem::new(SharedString::from(label)).on_click(
                                        move |_ev, window, cx| {
                                            // 预设删除不可撤销，先弹二次确认
                                            let weak = weak2.clone();
                                            let pid = id.clone();
                                            let pname = name.clone();
                                            window.open_alert_dialog(cx, move |alert, _, _| {
                                                let weak = weak.clone();
                                                let pid = pid.clone();
                                                alert
                                                    .title("删除预设")
                                                    .description(format!(
                                                        "确定删除预设「{pname}」吗？此操作不可撤销。"
                                                    ))
                                                    .button_props(
                                                        DialogButtonProps::default()
                                                            .show_cancel(true)
                                                            .cancel_text("取消")
                                                            .ok_text("删除")
                                                            .ok_variant(ButtonVariant::Danger),
                                                    )
                                                    .on_ok(move |_, _, cx| {
                                                        weak.update(cx, |view, cx| {
                                                            if let Some(app) = view.app.upgrade() {
                                                                app.update(cx, |app, cx| {
                                                                    app.delete_publish_preset(&pid);
                                                                    cx.notify();
                                                                });
                                                            }
                                                            cx.notify();
                                                        })
                                                        .ok();
                                                        true
                                                    })
                                            });
                                        },
                                    ),
                                );
                            }
                            submenu
                        },
                    );
                }
                menu
            })
    }

    /// 打开"保存为预设"命名对话框，保存当前发布参数。
    fn open_save_preset_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        name_input: Entity<InputState>,
    ) {
        // 预填默认名
        let topic = self.pub_topic.read(cx).value().to_string();
        let default_name = if topic.is_empty() {
            "新预设".to_string()
        } else {
            topic.chars().take(20).collect()
        };
        name_input.update(cx, |s, cx| s.set_value(default_name, window, cx));

        let weak = cx.weak_entity();
        let name_for_ok = name_input.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .w(px(420.))
                .title("保存消息预设")
                .child(v_flex().gap_3().child(
                    crate::ui::widgets::field("预设名称", Input::new(&name_for_ok)),
                ))
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .w_full()
                        .child(
                            Button::new("preset-save-cancel")
                                .label("取消")
                                .outline()
                                .on_click(move |_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("preset-save-ok")
                                .label("保存")
                                .primary()
                                .on_click({
                                    let name_input = name_for_ok.clone();
                                    let weak = weak.clone();
                                    move |_ev, window, cx| {
                                        let name = name_input
                                            .read(cx)
                                            .value()
                                            .trim()
                                            .to_string();
                                        if name.is_empty() {
                                            window.push_notification(
                                                Notification::warning("请填写预设名称"),
                                                cx,
                                            );
                                            return;
                                        }
                                        // 复用 do_publish 的过期秒数校验：输入了但解析不出即拒绝
                                        let params = weak
                                            .read_with(cx, |view, cx| {
                                                let expiry_raw = view
                                                    .msg_expiry
                                                    .read(cx)
                                                    .value()
                                                    .to_string();
                                                (
                                                    view.collect_publish_params(cx),
                                                    expiry_raw,
                                                )
                                            })
                                            .ok();
                                        if let (Some(view), Some((params, expiry_raw))) =
                                            (weak.upgrade(), params)
                                        {
                                            if !expiry_raw.trim().is_empty()
                                                && params.message_expiry_interval.is_none()
                                            {
                                                window.push_notification(
                                                    Notification::warning(
                                                        "消息过期须为非负整数（秒）",
                                                    ),
                                                    cx,
                                                );
                                                return;
                                            }
                                            if let Some(app) = view.read(cx).app.upgrade() {
                                                // 同名即覆盖，提示语区分以明确发生了什么
                                                let existed = app
                                                    .read(cx)
                                                    .presets
                                                    .iter()
                                                    .any(|p| p.name == name);
                                                app.update(cx, |app, cx| {
                                                    app.upsert_publish_preset(name.clone(), params);
                                                    cx.notify();
                                                });
                                                let msg = if existed {
                                                    format!("已覆盖预设「{name}」")
                                                } else {
                                                    "预设已保存".to_string()
                                                };
                                                window.push_notification(
                                                    Notification::success(msg),
                                                    cx,
                                                );
                                            }
                                        }
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
        });
    }
}
