//! 订阅：表单操作、订阅列表渲染与主题配色。


use gpui_kit::component::button::{ Button };
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{ PopupMenuItem };
use gpui_kit::component::select::Select;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ h_flex, notification::Notification, v_flex,  };
use crate::ui::IconName;
use gpui_kit::{ div, hsla, px, Context, IntoElement, SharedString, Window,  };
use gpui_kit::component::IndexPath;

use crate::model::{ SubscribeOptions, Subscription,  };
use crate::ui::widgets::field;

use super::*;
use crate::ui::i18n;

impl ConnectionView {
    /// 订阅面板收起后的窄 rail：抽屉把手。
    pub(super) fn render_subs_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w(px(32.))
            .h_full()
            .flex_shrink_0()
            .items_center()
            .pt_2()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().sidebar_foreground)
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("subs-rail-expand")
                    .icon(IconName::PanelLeftOpen)
                    .ghost()
                    .small()
                    .tooltip(i18n::t("展开订阅面板"))
                    .accessibility_label(i18n::t("展开订阅面板"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.subs_collapsed = false;
                        cx.notify();
                    })),
            )
    }

    /// 订阅按 (连接, 主题) 唯一：先摘掉旧条目再走 `add_subscription`，
    /// 借它入列 + `save_subscriptions` 落盘的逻辑完成 upsert（storage 字段是私有的）。
    /// 落盘顺序以 add 时为准，入列后移回原位以保持界面顺序稳定。
    fn upsert_subscription(&self, sub: Subscription, cx: &mut Context<Self>) {
        let Some(app_entity) = self.app.upgrade() else {
            return;
        };
        app_entity.update(cx, |app, cx| {
            let slot = app
                .subscriptions
                .iter()
                .position(|s| s.connection_id == sub.connection_id && s.topic == sub.topic);
            app.subscriptions.retain(|s| {
                !(s.connection_id == sub.connection_id && s.topic == sub.topic)
            });
            let new_id = sub.id.clone();
            app.add_subscription(sub);
            if let Some(pos) = slot
                && let Some(from) = app.subscriptions.iter().position(|s| s.id == new_id)
            {
                let moved = app.subscriptions.remove(from);
                app.subscriptions.insert(pos.min(app.subscriptions.len()), moved);
            }
            cx.notify();
        });
        cx.notify();
    }

    fn set_subscription_color(&mut self, sub_id: &str, hue: Option<f32>, cx: &mut Context<Self>) {
        let sub = self
            .with_app(cx, |app| {
                app.subscriptions
                    .iter()
                    .find(|s| s.id == sub_id)
                    .cloned()
            })
            .flatten();
        if let Some(mut sub) = sub {
            sub.color = hue;
            self.upsert_subscription(sub, cx);
        }
    }

    /// 启停订阅：置灰/恢复行显示，并在已连接时对引擎退订/重订。
    fn toggle_subscription_enabled(&mut self, sub_id: &str, cx: &mut Context<Self>) {
        let Some(mut sub) = self
            .with_app(cx, |app| {
                app.subscriptions
                    .iter()
                    .find(|s| s.id == sub_id)
                    .cloned()
            })
            .flatten()
        else {
            return;
        };
        sub.enabled = !sub.enabled;
        let enabled = sub.enabled;
        let topic = sub.topic.clone();
        // 停用的订阅不再参与消息过滤，指向它的过滤一并清掉
        if !enabled && self.sub_filter.as_deref() == Some(topic.as_str()) {
            self.sub_filter = None;
        }
        let conn = self.conn_id.clone();
        self.upsert_subscription(sub.clone(), cx);
        if self.engine.is_connected(&conn) {
            if enabled {
                self.engine
                    .subscribe_with_options(conn, sub.clone(), SubscribeOptions::from(&sub));
            } else {
                self.engine.unsubscribe(conn, topic);
            }
        }
    }

    /// 回填输入区进入编辑态；旧订阅保留到提交时才替换，中途放弃不丢数据。
    fn start_edit_subscription(&mut self, sub: &Subscription, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(sub.topic.clone());
        self.sub_topic
            .update(cx, |s, cx| s.set_value(sub.topic.clone(), window, cx));
        let qos_idx = (sub.qos.min(2)) as usize;
        self.sub_qos.update(cx, |s, cx| {
            s.set_selected_index(Some(IndexPath::new(qos_idx)), window, cx)
        });
        let alias = sub.alias.clone().unwrap_or_default();
        self.sub_alias
            .update(cx, |s, cx| s.set_value(alias, window, cx));
        let ident = sub
            .sub_identifier
            .map(|v| v.to_string())
            .unwrap_or_default();
        self.sub_identifier
            .update(cx, |s, cx| s.set_value(ident, window, cx));
        self.sub_no_local = sub.no_local;
        self.sub_rap = sub.retain_as_published;
        let rh_idx = sub.retain_handling.min(2) as usize;
        self.sub_retain_handling.update(cx, |s, cx| {
            s.set_selected_index(Some(IndexPath::new(rh_idx)), window, cx)
        });
        self.sub_show_advanced = true;
        cx.notify();
    }

    /// 重置订阅表单（主题/别名/高级字段）：新增提交与取消编辑共用，
    /// 避免高级选项残留到下一次订阅。
    fn reset_subscribe_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sub_topic
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.sub_alias
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.sub_identifier
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.sub_no_local = false;
        self.sub_rap = false;
        self.sub_retain_handling.update(cx, |s, cx| {
            s.set_selected_index(Some(IndexPath::new(0)), window, cx)
        });
    }

    // ── 订阅 / 发布动作 ─────────────────────────────────────────────────────

    fn do_subscribe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.sub_topic.read(cx).value().to_string();
        let topics = split_topic_list(&raw);
        if topics.is_empty() {
            window.push_notification(Notification::warning(i18n::t("订阅主题不能为空")), cx);
            return;
        }
        let qos = self.sub_qos.read(cx).selected_value().copied().unwrap_or(0) as u8;
        let alias_raw = self.sub_alias.read(cx).value().to_string();
        let alias = {
            let t = alias_raw.trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        let is_v5 = self.is_v5(cx);
        // v5 选项只对 v5 连接生效，v4 下不读取（避免无效输入阻断订阅）
        let mut sub_identifier = None;
        if is_v5 {
            let ident = self.sub_identifier.read(cx).value().to_string();
            let t = ident.trim();
            if !t.is_empty() {
                match t.parse::<u32>() {
                    Ok(v) if v > 0 => sub_identifier = Some(v),
                    _ => {
                        window.push_notification(
                            Notification::warning(i18n::t("订阅标识符须为正整数")),
                            cx,
                        );
                        return;
                    }
                }
            }
        }
        let no_local = self.sub_no_local;
        let rap = self.sub_rap;
        let retain_handling = self
            .sub_retain_handling
            .read(cx)
            .selected_value()
            .copied()
            .unwrap_or(0) as u8;

        let connected = self.engine.is_connected(&self.conn_id);
        let conn = self.conn_id.clone();

        // 记录编辑来源：编辑态提交后以目标订阅回填，保住编辑上下文
        let was_editing = self.editing.clone();
        // 改名编辑时旧记录随后会被删除，先取出供新主题承接 id/color/enabled
        let editing_old = was_editing.as_ref().and_then(|old_topic| {
            self.with_app(cx, |app| {
                app.subscriptions
                    .iter()
                    .find(|s| s.connection_id == conn && s.topic == *old_topic)
                    .cloned()
            })
            .flatten()
        });

        // 编辑提交：旧主题不在新列表里时先移除（含引擎退订），新主题由下方 upsert 覆盖
        if let Some(old_topic) = self.editing.take()
            && !topics.contains(&old_topic)
        {
            if self.sub_filter.as_deref() == Some(old_topic.as_str()) {
                self.sub_filter = None;
            }
            if let Some(app_entity) = self.app.upgrade() {
                app_entity.update(cx, |app, cx| {
                    app.remove_subscription(&conn, &old_topic);
                    cx.notify();
                });
            }
        }

        for topic in &topics {
            // 按 (连接, 主题) 查旧记录：继承 id/color/enabled，避免重复提交把备注色清掉、
            // 把停用中的订阅静默重启
            let existing = self
                .with_app(cx, |app| {
                    app.subscriptions
                        .iter()
                        .find(|s| s.connection_id == conn && s.topic == *topic)
                        .cloned()
                })
                .flatten();
            let sub = match existing {
                Some(mut old) => {
                    old.qos = qos;
                    old.alias = alias.clone();
                    if is_v5 {
                        old.sub_identifier = sub_identifier;
                        old.no_local = no_local;
                        old.retain_as_published = rap;
                        old.retain_handling = retain_handling;
                    }
                    old
                }
                None => {
                    // 改名编辑的单主题提交：新主题承接旧记录身份与备注，避免改名丢数据
                    if topics.len() == 1 && let Some(mut old) = editing_old.clone() {
                        old.topic = topic.clone();
                        old.qos = qos;
                        old.alias = alias.clone();
                        if is_v5 {
                            old.sub_identifier = sub_identifier;
                            old.no_local = no_local;
                            old.retain_as_published = rap;
                            old.retain_handling = retain_handling;
                        }
                        old
                    } else {
                        let mut sub = Subscription::new(conn.clone(), topic.clone(), qos);
                        sub.alias = alias.clone();
                        if is_v5 {
                            sub.sub_identifier = sub_identifier;
                            sub.no_local = no_local;
                            sub.retain_as_published = rap;
                            sub.retain_handling = retain_handling;
                        }
                        sub
                    }
                }
            };
            // 继承到的停用订阅保持退订状态，不因重新提交被拉起
            let keep_disabled = !sub.enabled;
            self.upsert_subscription(sub.clone(), cx);
            if connected && !keep_disabled {
                self.engine.subscribe_with_options(
                    conn.clone(),
                    sub.clone(),
                    SubscribeOptions::from(&sub),
                );
            }
        }
        if !connected {
            let hint = if self
                .config(cx)
                .map(|c| c.auto_resubscribe)
                .unwrap_or(true)
            {
                i18n::t("未连接：订阅已保存，连接后自动恢复")
            } else {
                i18n::t("未连接：订阅已保存")
            };
            window.push_notification(Notification::warning(hint), cx);
        }

        // 编辑态：以刚保存的目标订阅回填表单继续保留上下文；
        // 新增态：清空主题/别名并重置高级字段，避免残留到下一次订阅
        if was_editing.is_some() && topics.len() == 1 {
            let target = topics[0].clone();
            let saved = self
                .with_app(cx, |app| {
                    app.subscriptions
                        .iter()
                        .find(|s| s.connection_id == conn && s.topic == target)
                        .cloned()
                })
                .flatten();
            if let Some(saved) = saved {
                self.start_edit_subscription(&saved, window, cx);
            } else {
                self.editing = None;
                self.reset_subscribe_form(window, cx);
            }
        } else {
            self.editing = None;
            self.reset_subscribe_form(window, cx);
        }
        cx.notify();
    }

pub(super) fn render_subscribe_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let sub_topic = self.sub_topic.clone();
        let sub_qos = self.sub_qos.clone();
        let sub_alias = self.sub_alias.clone();
        let is_v5 = self.is_v5(cx);
        let editing_topic = self.editing.clone();
        let editing = editing_topic.is_some();
        let show_advanced = self.sub_show_advanced && is_v5;

        let mut bar = v_flex()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary);
        bar = bar.child(
            h_flex()
                .gap_2()
                .px_3()
                .h_12()
                .items_center()
                // 编辑态显式提示目标主题，避免被误当成新增提交
                .when_some(editing_topic.clone(), |h, topic| {
                    h.child(
                        div()
                            .px_1p5()
                            .py_0p5()
                            .rounded_md()
                            .bg(cx.theme().warning.alpha(0.15))
                            .text_xs()
                            .text_color(cx.theme().warning)
                            .child(i18n::tf("正在编辑：{topic}", &[("topic", &topic)])),
                    )
                })
                .child(
                    div()
                        .id(SharedString::from(format!("sub-topic-wrap-{}", self.conn_id)))
                        .flex_1()
                        .min_w(px(0.))
                        // Enter 直接提交订阅；Esc 退出编辑态或清空输入，
                        // 调试时不必为每次订阅伸手点按钮
                        .on_key_down(cx.listener(
                            |this, ev: &gpui_kit::KeyDownEvent, window, cx| {
                                if ev.keystroke.key == "enter"
                                    && !ev.keystroke.modifiers.control
                                    && !ev.keystroke.modifiers.alt
                                {
                                    cx.stop_propagation();
                                    this.do_subscribe(window, cx);
                                } else if ev.keystroke.key == "escape" {
                                    cx.stop_propagation();
                                    if this.editing.is_some() {
                                        this.editing = None;
                                        this.reset_subscribe_form(window, cx);
                                    } else {
                                        this.sub_topic
                                            .update(cx, |s, cx| s.set_value("", window, cx));
                                    }
                                    cx.notify();
                                }
                            },
                        ))
                        .child(Input::new(&sub_topic).small()),
                )
                .child(div().w(px(96.)).child(Select::new(&sub_qos).small()))
                .child(div().w(px(112.)).child(Input::new(&sub_alias).small()))
                .when(is_v5, |h| {
                    h.child(
                        Button::new(SharedString::from(format!("sub-adv-{}", self.conn_id)))
                            .icon(IconName::SlidersHorizontal)
                            .label(i18n::t("高级"))
                            .ghost()
                            .small()
                            .when(show_advanced, |b| b.selected(true))
                            .tooltip(i18n::t("MQTT 5 订阅选项"))
                            .accessibility_label(i18n::t("MQTT 5 订阅选项"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sub_show_advanced = !this.sub_show_advanced;
                                cx.notify();
                            })),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!("subscribe-{}", self.conn_id)))
                        .icon(if editing { IconName::Check } else { IconName::Plus })
                        .label(if editing { i18n::t("更新") } else { i18n::t("订阅") })
                        // 编辑用 warning 色与新增的 primary 区分
                        .when(editing, |b| b.warning())
                        .when(!editing, |b| b.primary())
                        .small()
                        .on_click(cx.listener(|this, _, window, cx| this.do_subscribe(window, cx))),
                )
                .when(editing, |h| {
                    h.child(
                        Button::new(SharedString::from(format!("sub-edit-cancel-{}", self.conn_id)))
                            .icon(IconName::Close)
                            .ghost()
                            .small()
                            .tooltip(i18n::t("取消编辑"))
                            .accessibility_label(i18n::t("取消编辑"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.editing = None;
                                // 清空回填到输入框的主题/别名/高级字段，回到新增态
                                this.reset_subscribe_form(window, cx);
                                cx.notify();
                            })),
                    )
                }),
        );

        if show_advanced {
            let sub_identifier = self.sub_identifier.clone();
            let sub_no_local = self.sub_no_local;
            let sub_rap = self.sub_rap;
            let sub_retain_handling = self.sub_retain_handling.clone();
            bar = bar.child(
                h_flex()
                    .gap_4()
                    .px_3()
                    .pb_2()
                    .items_start()
                    .child(
                        div()
                            .w(px(150.))
                            .child(field("订阅标识符", Input::new(&sub_identifier).small())),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .pt_4()
                            .child(Switch::new("sub-no-local").checked(sub_no_local).on_change(
                                cx.listener(|this, v, _, cx| {
                                    this.sub_no_local = *v;
                                    cx.notify();
                                }),
                            ))
                            .child(div().text_xs().child(i18n::t("No Local"))),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .pt_4()
                            .child(Switch::new("sub-rap").checked(sub_rap).on_change(
                                cx.listener(|this, v, _, cx| {
                                    this.sub_rap = *v;
                                    cx.notify();
                                }),
                            ))
                            .child(div().text_xs().child(i18n::t("Retain As Published"))),
                    )
                    .child(
                        div().w(px(150.)).child(field(
                            i18n::t("Retain Handling"),
                            Select::new(&sub_retain_handling).small(),
                        )),
                    ),
            );
        }
        bar
    }

pub(super) fn render_subscriptions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hover_bg = cx.theme().muted;
        let muted_fg = cx.theme().muted_foreground;
        let conn = self.conn_id.clone();
        let subs: Vec<Subscription> = self
            .with_app(cx, |app| {
                app.subscriptions
                    .iter()
                    .filter(|s| s.connection_id == conn)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let active_filter = self.sub_filter.clone();

        let mut list = v_flex().gap_0p5();
        if subs.is_empty() {
            list = list.child(
                v_flex()
                    .p_3()
                    .gap_2()
                    .items_center()
                    .child(
                        gpui_kit::component::Icon::new(IconName::Hash)
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .text_center()
                            .child(i18n::t("订阅主题后，消息会显示在右侧")),
                    ),
            );
        }
        for sub in subs {
            let enabled = sub.enabled;
            let is_active = enabled && active_filter.as_deref() == Some(sub.topic.as_str());
            let topic = sub.topic.clone();
            let sub_id = sub.id.clone();
            let alias = sub.alias.clone().filter(|a| !a.trim().is_empty());
            let primary = alias.clone().unwrap_or_else(|| topic.clone());

            // 颜色按钮：已设色显示色点，未设显示灰色「+」
            let color_btn = {
                let weak_menu = cx.weak_entity();
                let sid = sub_id.clone();
                let trigger_id = SharedString::from(format!("sub-color-{}", sub_id));
                let trigger = if let Some(hue) = sub.color {
                    Button::new(trigger_id)
                        .ghost()
                        .xsmall()
                        .tooltip(i18n::t("订阅颜色"))
                        .accessibility_label(i18n::t("订阅颜色"))
                        .child(
                            div()
                                .size(px(11.))
                                .rounded_full()
                                .bg(hue_color(hue))
                                .border_1()
                                .border_color(cx.theme().border),
                        )
                } else {
                    Button::new(trigger_id)
                        .ghost()
                        .xsmall()
                        .tooltip(i18n::t("订阅颜色"))
                        .accessibility_label(i18n::t("订阅颜色"))
                        .child(
                            div()
                                .size(px(11.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(cx.theme().muted)
                                .text_color(muted_fg)
                                .text_xs()
                                .child("+"),
                        )
                };
                trigger.dropdown_menu(move |mut menu, _, _| {
                    for (hue, name) in PRESET_HUES {
                        let weak = weak_menu.clone();
                        let sid = sid.clone();
                        let swatch = hue_color(hue);
                        // 菜单只显示颜色名，角度数值对用户没有意义
                        let label = SharedString::from(name);
                        menu = menu.item(
                            PopupMenuItem::element(move |_, _| {
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .size(px(10.))
                                            .rounded_full()
                                            .bg(swatch)
                                            .border_1()
                                            .border_color(hsla(0., 0., 1., 0.6)),
                                    )
                                    .child(div().text_sm().child(label.clone()))
                            })
                            .on_click(move |_, _, cx| {
                                weak
                                    .update(cx, |view, cx| {
                                        view.set_subscription_color(&sid, Some(hue), cx);
                                    })
                                    .ok();
                            }),
                        );
                    }
                    menu = menu.separator();
                    let weak_rand = weak_menu.clone();
                    let sid_rand = sid.clone();
                    menu = menu.item(PopupMenuItem::new(i18n::t("随机颜色")).on_click(move |_, _, cx| {
                        let hue = (uuid::Uuid::new_v4().as_u128() % 360) as f32;
                        weak_rand
                            .update(cx, |view, cx| {
                                view.set_subscription_color(&sid_rand, Some(hue), cx);
                            })
                            .ok();
                    }));
                    let weak_clear = weak_menu.clone();
                    let sid_clear = sid.clone();
                    menu = menu.item(PopupMenuItem::new(i18n::t("清除颜色")).on_click(move |_, _, cx| {
                        weak_clear
                            .update(cx, |view, cx| {
                                view.set_subscription_color(&sid_clear, None, cx);
                            })
                            .ok();
                    }));
                    menu
                })
            };

            let eye_sid = sub_id.clone();
            let eye_btn = Button::new(SharedString::from(format!("sub-eye-{}", sub_id)))
                .icon(if enabled { IconName::Eye } else { IconName::EyeOff })
                .ghost()
                .xsmall()
                .tooltip(if enabled { i18n::t("停用订阅") } else { i18n::t("启用订阅") })
                .accessibility_label(if enabled { i18n::t("停用订阅") } else { i18n::t("启用订阅") })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_subscription_enabled(&eye_sid, cx);
                }));

            let edit_sid = sub_id.clone();
            let edit_btn = Button::new(SharedString::from(format!("sub-edit-{}", sub_id)))
                .icon(IconName::Pencil)
                .ghost()
                .xsmall()
                .tooltip(i18n::t("编辑订阅"))
                .accessibility_label(i18n::t("编辑订阅"))
                .on_click(cx.listener(move |this, _, window, cx| {
                    let sub = this
                        .with_app(cx, |app| {
                            app.subscriptions
                                .iter()
                                .find(|s| s.id == edit_sid)
                                .cloned()
                        })
                        .flatten();
                    if let Some(sub) = sub {
                        this.start_edit_subscription(&sub, window, cx);
                    }
                }));

            let del_cid = self.conn_id.clone();
            let del_topic = topic.clone();
            let del_btn = Button::new(SharedString::from(format!("sub-del-{}", sub_id)))
                .icon(IconName::Close)
                .ghost()
                .xsmall()
                .tooltip(i18n::t("取消订阅"))
                .accessibility_label(i18n::t("取消订阅"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    // 若删除的是当前过滤主题，一并清掉消息过滤
                    if this.sub_filter.as_deref() == Some(del_topic.as_str()) {
                        this.sub_filter = None;
                    }
                    if let Some(app) = this.app.upgrade() {
                        app.update(cx, |app, cx| {
                            app.remove_subscription(&del_cid, &del_topic);
                            cx.notify();
                        });
                    }
                    // 只有删除的是正在编辑的主题才退出编辑态，编辑别的不受影响
                    if this.editing.as_deref() == Some(del_topic.as_str()) {
                        this.editing = None;
                    }
                    cx.notify();
                }));

            // 点击区（非按钮）：切换按该主题过滤消息流；禁用的订阅不参与
            let filter_topic = topic.clone();
            let click_zone = h_flex()
                .id(SharedString::from(format!("sub-click-{}", sub_id)))
                .flex_1()
                .min_w(px(0.))
                .when(enabled, |z| {
                    z.cursor_pointer().on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.sub_filter = if this.sub_filter.as_deref()
                                == Some(filter_topic.as_str())
                            {
                                None
                            } else {
                                Some(filter_topic.clone())
                            };
                            cx.notify();
                        },
                    ))
                })
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .gap_0p5()
                        .child(
                            div()
                                .text_sm()
                                .overflow_hidden()
                                .text_ellipsis()
                                .when(!enabled, |t| t.text_color(muted_fg))
                                .child(primary),
                        )
                        .when_some(alias, |v, _| {
                            v.child(
                                div()
                                    .text_xs()
                                    .text_color(muted_fg)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(topic.clone()),
                            )
                        }),
                );

            list = list.child(
                h_flex()
                    .id(SharedString::from(format!("sub-{}-{}", self.conn_id, sub.id)))
                    .gap_1p5()
                    .items_center()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .when(is_active, |t| t.bg(cx.theme().secondary))
                    .when(!is_active, |t| t.hover(move |this| this.bg(hover_bg)))
                    .when(!enabled, |t| t.opacity(0.55))
                    .child(color_btn)
                    .child(click_zone)
                    .child(
                        div()
                            .text_xs()
                            .px_1()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_color(muted_fg)
                            .child(format!("Q{}", sub.qos)),
                    )
                    .child(eye_btn)
                    .child(edit_btn)
                    .child(del_btn),
            );
        }

        v_flex()
            .w(px(248.))
            .flex_shrink_0()
            .h_full()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().sidebar_foreground)
            // 深色主题下订阅面板与消息区同色，补一条分隔线保持边界可辨
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .h_11()
                    .px_3()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().font_semibold().child(i18n::t("订阅")))
                    .child(
                        Button::new("subs-collapse")
                            .icon(IconName::PanelLeftClose)
                            .ghost()
                            .xsmall()
                            .tooltip(i18n::t("收起订阅面板"))
                            .accessibility_label(i18n::t("收起订阅面板"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.subs_collapsed = true;
                                cx.notify();
                            })),
                    ),
            )
            .child(div().flex_1().overflow_y_scrollbar().px_2().child(list))
    }
}
