//! 模板变量面板：扫描 topic/payload 占位符并管理输入行。


use gpui_kit::component::button::{ Button };
use gpui_kit::component::input::{ Input, InputEvent, InputState };
use gpui_kit::component::{ h_flex, notification::Notification, v_flex,  };
use crate::ui::IconName;
use gpui_kit::{ div, px, Context, IntoElement, Window,  };

use crate::model::{ render_template, GlobalVariable,  };

use super::*;
use crate::ui::i18n;

impl ConnectionView {
    /// 输入框创建需要 Window，因此只能在按钮回调里调用（无法在 render 中同步）。
pub(super) fn sync_var_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let topic = self.pub_topic.read(cx).value().to_string();
        let payload = self.payload.read(cx).value().to_string();
        let keys = extract_placeholder_keys(&topic, &payload);
        if keys.is_empty() {
            self.show_vars = false;
            window.push_notification(
                Notification::warning(i18n::t("var.no_reference")),
                cx,
            );
            cx.notify();
            return;
        }
        let vars = self.with_app(cx, |a| a.variables.clone()).unwrap_or_default();
        // 移除已不存在的 key（保留仍存在的行，避免丢未保存的输入）
        self.var_rows.retain(|(k, _, _)| keys.contains(k));
        for key in keys {
            if self.var_rows.iter().any(|(k, ..)| k == &key) {
                continue;
            }
            let initial = vars
                .iter()
                .find(|v| v.key == key)
                .map(|v| v.value.clone())
                .unwrap_or_default();
            let input = cx.new(|cx| {
                let mut s = InputState::new(window, cx).placeholder(i18n::t("var.value"));
                // set_value 不会触发 InputEvent::Change，不会误写回
                if !initial.is_empty() {
                    s.set_value(initial.as_str(), window, cx);
                }
                s
            });
            // 编辑即写回全局变量（含持久化），预览与发布随之生效
            let key_for_sub = key.clone();
            let sub = cx.subscribe(&input, move |this, entity, ev: &InputEvent, cx| {
                if !matches!(ev, InputEvent::Change) {
                    return;
                }
                let val = entity.read(cx).value().to_string();
                let key = key_for_sub.clone();
                let Some(app) = this.app.upgrade() else {
                    return;
                };
                app.update(cx, |a, cx| {
                    match a.variables.iter_mut().find(|v| v.key == key) {
                        Some(v) => {
                            if v.value == val {
                                return;
                            }
                            v.value = val;
                        }
                        None => a.variables.push(GlobalVariable { key, value: val }),
                    }
                    let snapshot = a.variables.clone();
                    a.save_variables(snapshot);
                    cx.notify();
                });
            });
            self.var_rows.push((key, input, sub));
        }
        self.show_vars = true;
        cx.notify();
    }

    /// 变量绑定面板：逐 key 编辑 + 实时渲染预览。
pub(super) fn render_vars_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let vars = self.with_app(cx, |a| a.variables.clone()).unwrap_or_default();
        let topic_raw = self.pub_topic.read(cx).value().to_string();
        let payload_raw = self.payload.read(cx).value().to_string();
        let topic_rendered = render_template(&topic_raw, &vars);
        let payload_rendered = render_template(&payload_raw, &vars);
        let payload_preview: String = payload_rendered.chars().take(80).collect();
        let muted = cx.theme().muted_foreground;

        let mut rows = v_flex().gap_1();
        for (key, input, _) in &self.var_rows {
            rows = rows.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .w(px(160.))
                            .min_w(px(0.))
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{{{{{key}}}}}")),
                    )
                    .child(div().flex_1().min_w(px(0.)).child(Input::new(input).small())),
            );
        }

        v_flex()
            .gap_2()
            .w_full()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .child(i18n::tf(
                                "var.section",
                                &[("n", &self.var_rows.len())],
                            )),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("vars-refresh")
                            .icon(IconName::RefreshCw)
                            .ghost()
                            .xsmall()
                            .tooltip(i18n::t("var.reextract"))
                            .accessibility_label(i18n::t("var.reextract"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.sync_var_rows(window, cx);
                            })),
                    )
                    .child(
                        Button::new("vars-collapse")
                            .icon(IconName::ChevronUp)
                            .ghost()
                            .xsmall()
                            .tooltip(i18n::t("common.collapse"))
                            .accessibility_label(i18n::t("common.collapse"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_vars = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(rows)
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(i18n::t("var.builtin_inline")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(i18n::tf(
                                "var.preview",
                                &[("tp", &topic_rendered), ("pl", &payload_preview)],
                            )),
            )
    }

}
