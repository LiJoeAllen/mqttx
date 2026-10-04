//! 消息区：顶栏过滤/工具条、消息流渲染、单条消息行与日志视图。


use gpui_kit::component::button::{ Button, ButtonGroup, ButtonVariant };
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::Input;
use gpui_kit::component::{ h_flex, notification::Notification, v_flex,  };
use crate::ui::IconName;
use gpui_kit::{ div, hsla, px, ClipboardItem, Context, IntoElement, SharedString,  };

use crate::model::{ render_will_templates, ConnectionStatus, Direction, LogLevel, MqttRecord, Subscription, MAX_PAYLOAD_RETAIN,  };
use crate::ui::widgets::format_time;

use super::*;
use crate::ui::i18n;

impl ConnectionView {
pub(super) fn render_top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.conn_id.clone();
        let (name, status, is_v5, error) = self
            .with_app(cx, |app| {
                let cfg = app.connections.iter().find(|c| c.id == id);
                (
                    cfg.map(|c| c.name.clone()).unwrap_or_else(|| id.clone()),
                    app.statuses.get(&id).copied().unwrap_or(ConnectionStatus::Disconnected),
                    cfg.map(|c| c.protocol.is_v5()).unwrap_or(true),
                    app.errors.get(&id).cloned(),
                )
            })
            .unwrap_or((id.clone(), ConnectionStatus::Disconnected, true, None));
        let connected = matches!(status, ConnectionStatus::Connected);
        let connecting = matches!(status, ConnectionStatus::Connecting);
        let _ = is_v5;

        let status_text = i18n::t(status.label());
        let status_tone = match status {
            ConnectionStatus::Connected => cx.theme().success,
            ConnectionStatus::Connecting => cx.theme().warning,
            ConnectionStatus::Error => cx.theme().danger,
            ConnectionStatus::Disconnected => cx.theme().muted_foreground,
        };
        // Error 状态把失败原因展示出来，避免只有「错误」二字无从排查
        let error_hint = matches!(status, ConnectionStatus::Error)
            .then(|| error.clone())
            .flatten();

        h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .px_3()
            .h_12()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().size(px(8.)).rounded_full().bg(status_tone))
            .child(div().text_sm().font_semibold().child(name))
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .px_1p5()
                    .py_0p5()
                    .rounded_md()
                    .bg(status_tone.alpha(0.12))
                    .child(div().size(px(5.)).rounded_full().bg(status_tone))
                    .child(
                        div()
                            .text_xs()
                            .text_color(status_tone)
                            .child(status_text),
                    ),
            )
            // Error 副文本展示失败原因（截断避免挤掉按钮）
            .when_some(error_hint.clone(), |h, e| {
                h.child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(e),
                )
            })
            .when(error_hint.is_none(), |h| h.child(div().flex_1()))
            .child(
                Button::new(SharedString::from(format!("conn-toggle-{}", self.conn_id)))
                    .icon(if connected {
                        IconName::Square
                    } else {
                        IconName::PlugZap
                    })
                    .label(if connected {
                        i18n::t("sidebar.disconnect")
                    } else if connecting {
                        i18n::t("status.connecting")
                    } else {
                        i18n::t("sidebar.connections")
                    })
                    .when(connected, |b| b.danger().ghost())
                    .when(!connected, |b| b.primary().ghost())
                    // Connecting 期间禁用，避免重复点击重启连接流程
                    .disabled(connecting)
                    .small()
                    .when_some(error_hint.clone(), |b, e| b.tooltip(e))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        // Connecting 期间（按钮禁用之外再兜一层）不响应，防止重复 connect 重启流程
                        let connecting = this
                            .with_app(cx, |app| {
                                app.statuses.get(&this.conn_id)
                                    == Some(&ConnectionStatus::Connecting)
                            })
                            .unwrap_or(false);
                        if connecting {
                            return;
                        }
                        let cfg = this.config(cx);
                        if let Some(mut cfg) = cfg {
                            if this.engine.is_connected(&cfg.id) {
                                this.engine.close(&cfg.id, true);
                            } else {
                                // 遗嘱 {{变量}} 在每次连接时按当前全局变量求值
                                let vars = this
                                    .with_app(cx, |app| app.variables.clone())
                                    .unwrap_or_default();
                                render_will_templates(&mut cfg, &vars);
                                this.engine.connect(cfg);
                            }
                        }
                    })),
            )
    }

    fn render_message_row(
        &self,
        record: &MqttRecord,
        sub_color: Option<f32>,
        show_millis: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let seq = record.seq;
        let expanded = self.expanded == Some(seq);
        let received = matches!(record.direction, Direction::Received);
        let accent = if received {
            cx.theme().primary
        } else {
            cx.theme().success
        };
        let hover_bg = cx.theme().muted;
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let mono = cx.theme().mono_font_family.clone();
        let direction_label = if received { i18n::t("dir.received") } else { i18n::t("dir.published") };

        let header = h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(accent.alpha(0.14))
                    .child(
                        gpui_kit::component::Icon::new(if received {
                            IconName::ArrowDown
                        } else {
                            IconName::ArrowUp
                        })
                        .size_3()
                        .text_color(accent),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_sm()
                    .font_medium()
                    .overflow_hidden()
                    .text_ellipsis()
                    .font_family(mono.clone())
                    // 匹配到订阅色时主题文字着色，否则保持默认前景色
                    .when_some(sub_color, |t, hue| t.text_color(hue_color(hue)))
                    .child(record.topic.to_string()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .font_family(mono.clone())
                    .child(direction_label),
            )
            .child(
                div()
                    .text_xs()
                    .px_1()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .text_color(muted)
                    .font_family(mono.clone())
                    .child(format!("Q{}", record.qos)),
            )
            .when(record.retain, |t| {
                t.child(
                    div()
                        .text_xs()
                        .px_1()
                        .rounded_sm()
                        .bg(cx.theme().warning.alpha(0.15))
                        .text_color(cx.theme().warning)
                        .child("retain"),
                )
            })
            .when(record.payload_truncated, |t| {
                t.child(
                    div()
                        .text_xs()
                        .px_1()
                        .rounded_sm()
                        .bg(cx.theme().warning.alpha(0.15))
                        .text_color(cx.theme().warning)
                        .child(i18n::tf("msg.truncated", &[("n", &(MAX_PAYLOAD_RETAIN / 1024))])),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .font_family(mono.clone())
                    .child(format_time(record.timestamp, show_millis)),
            );

        let mut body = v_flex()
            .flex_1()
            .min_w(px(0.))
            .px_3()
            .py_2()
            .child(header)
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .pt_0p5()
                    .font_family(mono.clone())
                    // 预览在入环形缓冲时已算好（JSON 美化/压平+截断），此处零解析
                    .child(SharedString::from(record.preview.clone())),
            );

        if expanded {
            let fmt = self.detail_format;
            let detail_text = format_payload_detail(&record.payload, record.raw_bytes.as_deref(), fmt);
            let mut details = v_flex().gap_1p5().pt_1();

            // 格式切换 + 复制按钮；stop_propagation 避免点击时又收起整行
            let fmt_group = ButtonGroup::new(SharedString::from(format!("msg-fmt-{}", seq)))
                .compact()
                .children(
                    [DetailFormat::Auto, DetailFormat::Text, DetailFormat::Hex, DetailFormat::Base64]
                        .into_iter()
                        .map(|f| {
                            Button::new(SharedString::from(format!(
                                "msg-fmt-btn-{}-{}",
                                seq,
                                f.index()
                            )))
                            .label(i18n::t(f.label()))
                            .small()
                            .selected(f == fmt)
                        }),
                )
                .on_click(cx.listener(|this, ixs: &Vec<usize>, _, cx| {
                    cx.stop_propagation();
                    if let Some(&i) = ixs.first() {
                        this.detail_format = DetailFormat::from_index(i);
                        cx.notify();
                    }
                }));

            let copy_topic = record.topic.clone();
            let copy_payload = record.payload.clone();
            let copy_detail = details_copy_text(record, show_millis);
            let detail_toolbar = h_flex()
                .gap_1()
                .items_center()
                .child(fmt_group)
                .child(div().flex_1())
                .child(
                    Button::new(SharedString::from(format!("msg-copy-topic-{}", seq)))
                        .icon(IconName::Copy)
                        .label(i18n::t("msg.topic"))
                        .ghost()
                        .xsmall()
                        .tooltip(i18n::t("msg.copy_topic"))
                        .accessibility_label(i18n::t("msg.copy_topic"))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_topic.to_string()));
                            window.push_notification(Notification::success(i18n::t("msg.topic_copied")), cx);
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("msg-copy-payload-{}", seq)))
                        .icon(IconName::Copy)
                        .label(i18n::t("msg.payload"))
                        .ghost()
                        .xsmall()
                        .tooltip(i18n::t("msg.copy_payload"))
                        .accessibility_label(i18n::t("msg.copy_payload"))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_payload.to_string()));
                            window.push_notification(Notification::success(i18n::t("msg.payload_copied")), cx);
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("msg-copy-detail-{}", seq)))
                        .icon(IconName::Copy)
                        .label(i18n::t("msg.details"))
                        .ghost()
                        .xsmall()
                        .tooltip(i18n::t("msg.copy_details"))
                        .accessibility_label(i18n::t("msg.copy_details"))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_detail.clone()));
                            window.push_notification(Notification::success(i18n::t("msg.details_copied")), cx);
                        }),
                );
            details = details.child(detail_toolbar).child(
                div()
                    .font_family(mono.clone())
                    .text_xs()
                    .p_2()
                    .rounded_md()
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(border)
                    .child(detail_text),
            );

            if !record.user_properties.is_empty() {
                let props: String = record
                    .user_properties
                    .iter()
                    .map(|(k, v)| format!("{k} = {v}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                details = details
                    .child(div().text_xs().font_semibold().child(i18n::t("preset.user_props")))
                    .child(
                        div()
                            .font_family(mono.clone())
                            .text_xs()
                            .child(props),
                    );
            }
            for (label, value) in [
                ("Content-Type", record.content_type.clone()),
                ("Response Topic", record.response_topic.clone()),
                ("Correlation Data", record.correlation_data.clone()),
                (
                    i18n::t("msg.expiry"),
                    record
                        .message_expiry_interval
                        .map(|v| i18n::tf("msg.seconds", &[("n", &v)])),
                ),
                (
                    "订阅标识符",
                    record.subscription_identifier.map(|v| v.to_string()),
                ),
            ] {
                if let Some(v) = value {
                    details = details.child(
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .child(div().text_color(muted).child(format!("{label}:")))
                            .child(div().font_family(mono.clone()).child(v)),
                    );
                }
            }
            body = body.child(
                div()
                    .id(SharedString::from(format!("msg-detail-{}", seq)))
                    .mt_1()
                    .p_2()
                    .rounded_lg()
                    .bg(cx.theme().muted)
                    // 详情区点击不冒泡到行折叠，保证文本可选中
                    .on_click(|_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(details),
            );
        }

        h_flex()
            .id(SharedString::from(format!(
                "msg-{}-{}",
                record.connection_id, record.seq
            )))
            .w_full()
            .border_b_1()
            .border_color(border)
            .hover(move |this| this.bg(hover_bg))
            .cursor_pointer()
            // 左侧色条：有订阅色时显示，否则透明以保持行内对齐；
            // 上下留 2px 间隙，避免相邻行的色条连成一整条竖线
            .child(
                div()
                    .w(px(3.))
                    .self_stretch()
                    .my_0p5()
                    .rounded_full()
                    .bg(sub_color.map(hue_color).unwrap_or(hsla(0., 0., 0., 0.))),
            )
            .child(body)
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.expanded == Some(seq) {
                    this.expanded = None;
                } else {
                    // 切换到另一条消息时格式回到 Auto，单条内折叠/展开仍记忆选择
                    if this.detail_owner != Some(seq) {
                        this.detail_format = DetailFormat::Auto;
                        this.detail_owner = Some(seq);
                    }
                    this.expanded = Some(seq);
                }
                cx.notify();
            }))
    }

pub(super) fn render_messages(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.filter.read(cx).value().to_lowercase();
        let conn = self.conn_id.clone();
        let dir = self.msg_dir;
        let sub_filter = self.sub_filter.clone();
        // 引用过滤 + 计数命中总数，仅克隆前 MAX_RENDERED_MESSAGES 条，避免全量拷贝
        let (records, subs, matched_total, total): (
            Vec<MqttRecord>,
            Vec<Subscription>,
            usize,
            usize,
        ) = self
            .with_app(cx, |app| {
                let subs: Vec<Subscription> = app
                    .subscriptions
                    .iter()
                    .filter(|s| s.connection_id == conn)
                    .cloned()
                    .collect();
                let mut matched = 0usize;
                let mut records: Vec<MqttRecord> = Vec::new();
                let total = app.messages.get(conn.as_str()).map_or(0, |m| m.len());
                if let Some(m) = app.messages.get(conn.as_str()) {
                    for r in m.iter().rev() {
                        // 方向 / 搜索 / 订阅过滤叠加，过滤后再截断保证最新消息优先
                        let dir_ok = match dir {
                            DirFilter::All => true,
                            DirFilter::Received => r.direction == Direction::Received,
                            DirFilter::Published => r.direction == Direction::Published,
                        };
                        let query_ok = query.is_empty()
                            || contains_ignore_case(&r.topic, &query)
                            || contains_ignore_case(&r.payload, &query);
                        let sub_ok = sub_filter
                            .as_deref()
                            .is_none_or(|f| topic_matches(f, &r.topic));
                        if dir_ok && query_ok && sub_ok {
                            matched += 1;
                            if records.len() < MAX_RENDERED_MESSAGES {
                                records.push(r.clone());
                            }
                        }
                    }
                }
                (records, subs, matched, total)
            })
            .unwrap_or_default();

        let mut body = v_flex().flex_1().overflow_y_scrollbar();
        if records.is_empty() {
            let filtering = !query.is_empty()
                || sub_filter.is_some()
                || dir != DirFilter::All;
            body = body.child(
                v_flex()
                    .h_full()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .p_4()
                    .child(
                        div()
                            .size(px(56.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(cx.theme().muted)
                            .child(
                                gpui_kit::component::Icon::new(IconName::MailOpen)
                                    .size_6()
                                    .text_color(cx.theme().muted_foreground),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(if filtering {
                                i18n::t("msg.no_match")
                            } else {
                                i18n::t("msg.empty")
                            }),
                    ),
            );
        } else {
            // 最新消息在最上方（与 MQTTX 一致，避免长列表需要手动滚底）
            let show_millis = self
                .with_app(cx, |app| app.settings.show_millis)
                .unwrap_or(true);
            for r in &records {
                let color = pick_subscription_color(&subs, &r.topic);
                body = body.child(self.render_message_row(r, color, show_millis, cx));
            }
        }

        // 方向过滤 seg
        let dir_group = ButtonGroup::new(SharedString::from(format!(
            "msg-dir-{}",
            self.conn_id
        )))
        .compact()
        .children([DirFilter::All, DirFilter::Received, DirFilter::Published].into_iter().map(
            |d| {
                Button::new(SharedString::from(format!(
                    "msg-dir-btn-{}-{}",
                    self.conn_id,
                    d.index()
                )))
                .label(i18n::t(d.label()))
                .small()
                .selected(d == dir)
            },
        ))
        .on_click(cx.listener(|this, ixs: &Vec<usize>, _, cx| {
            if let Some(&i) = ixs.first() {
                this.msg_dir = DirFilter::from_index(i);
                cx.notify();
            }
        }));

        // 订阅过滤 chip（点击订阅项激活）
        let filter_chip = self.sub_filter.clone().map(|topic| {
            h_flex()
                .gap_1()
                .items_center()
                .px_1p5()
                .py_0p5()
                .rounded_md()
                .bg(cx.theme().secondary)
                .max_w(px(180.))
                .child(
                    gpui_kit::component::Icon::new(IconName::ListFilter)
                        .xsmall()
                        .text_color(cx.theme().muted_foreground),
                )
                .child(
                    div()
                        .text_xs()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(topic),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "msg-filter-clear-{}",
                        self.conn_id
                    )))
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip(i18n::t("msg.clear_sub_filter"))
                    .accessibility_label(i18n::t("msg.clear_sub_filter"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sub_filter = None;
                        cx.notify();
                    })),
                )
        });

        v_flex()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .h_11()
                    .px_3()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(dir_group)
                    .child(
                        div()
                            .id(SharedString::from(format!("msg-filter-wrap-{}", self.conn_id)))
                            .flex_1()
                            .min_w(px(0.))
                            // Esc 清空过滤词：高频调试时比全选删除顺手
                            .on_key_down(cx.listener(
                                |this, ev: &gpui_kit::KeyDownEvent, window, cx| {
                                    if ev.keystroke.key == "escape" {
                                        cx.stop_propagation();
                                        this.filter
                                            .update(cx, |s, cx| s.set_value("", window, cx));
                                        cx.notify();
                                    }
                                },
                            ))
                            .child(Input::new(&self.filter).small().prefix(
                                gpui_kit::component::Icon::new(IconName::Search).small(),
                            )),
                    )
                    .children(filter_chip)
                    .child(
                        div()
                            .text_xs()
                            .px_1p5()
                            .text_color(cx.theme().muted_foreground)
                            // 计数显示过滤命中总数，不受渲染截断影响
                            .child(i18n::tf("msg.count", &[("n", &matched_total)])),
                    )
                    .when(matched_total > records.len(), |h| {
                        h.child(
                            div()
                                .text_xs()
                                .px_1p5()
                                .text_color(cx.theme().muted_foreground)
                                .child(i18n::tf(
                                "仅显示最新 {n} 条",
                                &[("n", &MAX_RENDERED_MESSAGES)],
                            )),
                        )
                    })
                    .child({
                        // 暂停滚动：高频消息流下锁定当前画面以便阅读/展开，
                        // 消息仍照常入环形缓冲，恢复后直接显示最新
                        let paused = self.paused;
                        Button::new(SharedString::from(format!(
                            "msg-pause-{}",
                            self.conn_id
                        )))
                        .icon(if paused {
                            IconName::Play
                        } else {
                            IconName::Pause
                        })
                        .label(if paused { i18n::t("common.resume") } else { i18n::t("common.pause") })
                        .ghost()
                        .small()
                        .when(paused, |b| b.selected(true))
                        .tooltip(if paused {
                            i18n::t("msg.resume_tooltip")
                        } else {
                            i18n::t("msg.pause_tooltip")
                        })
                        .accessibility_label(if paused {
                            i18n::t("common.resume")
                        } else {
                            i18n::t("common.pause")
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.paused = !this.paused;
                            cx.notify();
                        }))
                    })
                    .child({
                        let weak = cx.weak_entity();
                        let clear_id = self.conn_id.clone();
                        // 清空针对连接的全部消息（非过滤结果）：
                        // 禁用与否依据总数，过滤命中 0 条不应禁用
                        let empty = total == 0;
                        Button::new(SharedString::from(format!("msg-clear-{}", self.conn_id)))
                            .icon(IconName::Eraser)
                            .label(i18n::t("common.clear"))
                            .ghost()
                            .small()
                            .disabled(empty)
                            .tooltip(i18n::t("msg.clear_tooltip"))
                            .accessibility_label(i18n::t("msg.clear_tooltip"))
                            .on_click(move |_, window, cx| {
                                // 清空不可撤销，先弹二次确认
                                let weak = weak.clone();
                                let clear_id = clear_id.clone();
                                window.open_alert_dialog(cx, move |alert, _, _| {
                                    let weak = weak.clone();
                                    let clear_id = clear_id.clone();
                                    alert
                                        .title(i18n::t("msg.clear_title"))
                                        .description(i18n::t("msg.clear_confirm"))
                                        .button_props(
                                            DialogButtonProps::default()
                                                .show_cancel(true)
                                                .cancel_text(i18n::t("common.cancel"))
                                                .ok_text(i18n::t("common.clear"))
                                                .ok_variant(ButtonVariant::Danger),
                                        )
                                        .on_ok(move |_, _, cx| {
                                            weak.update(cx, |view, cx| {
                                                view.expanded = None;
                                                view.detail_owner = None;
                                                view.detail_format = DetailFormat::Auto;
                                                if let Some(app) = view.app.upgrade() {
                                                    app.update(cx, |app, cx| {
                                                        app.clear_messages(&clear_id);
                                                        cx.notify();
                                                    });
                                                }
                                                cx.notify();
                                            })
                                            .ok();
                                            true
                                        })
                                });
                            })
                    }),
            )
            .child(body)
    }

pub(super) fn render_logs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let conn = self.conn_id.clone();
        let show_millis = self
            .with_app(cx, |app| app.settings.show_millis)
            .unwrap_or(true);
        let logs: Vec<_> = self
            .with_app(cx, |app| {
                app.logs
                    .iter()
                    .rev()
                    .filter(|l| l.connection_id == conn)
                    // 与 app.rs 的 MAX_LOGS=3000 对齐，避免渲染截断早于存储上限
                    .take(MAX_RENDERED_LOGS)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        let mut body = v_flex().flex_1().overflow_y_scrollbar().p_2().gap_0p5();
        if logs.is_empty() {
            body = body.child(
                v_flex()
                    .p_4()
                    .gap_2()
                    .items_center()
                    .child(
                        gpui_kit::component::Icon::new(IconName::ScrollText)
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(i18n::t("msg.no_logs")),
                    ),
            );
        }
        let log_hover = cx.theme().muted;
        for l in logs {
            let color = match l.level {
                LogLevel::Error => cx.theme().danger,
                LogLevel::Warn => cx.theme().warning,
                LogLevel::Debug => cx.theme().muted_foreground,
                LogLevel::Info => cx.theme().foreground,
            };
            body = body.child(
                h_flex()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .hover(move |this| this.bg(log_hover))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(format_time(l.timestamp, show_millis)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .w_10()
                            .text_color(color)
                            .child(l.level.label()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .flex_1()
                            .min_w(px(0.))
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(format!("[{}] {}", l.event, l.message)),
                    ),
            );
        }
        v_flex()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .h_11()
                    .px_3()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(i18n::t("msg.log_title")),
                    ),
            )
            .child(body)
    }
}
