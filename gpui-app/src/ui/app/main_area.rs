//! 主区域：标签页栏与内容视图装配（含空态占位）。


use gpui_kit::component::button::{ Button };
use gpui_kit::component::tab::{ Tab, TabBar };
use gpui_kit::component::{ v_flex,  };
use gpui_kit::{ div, px, Context, IntoElement, SharedString, Window,  };

use crate::ui::IconName;

use super::*;

impl MqttXApp {
pub(super) fn render_main(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.open_tabs.is_empty() {
            // 父级 h_flex 交叉轴居中子项，容器必须 h_full 占满高度才不会整体下沉
            return v_flex()
                .flex_1()
                .h_full()
                .min_h(px(0.))
                .items_center()
                .justify_center()
                .gap_3()
                .child(crate::ui::logo::logo(
                    px(72.),
                    cx.theme().muted,
                    cx.theme().muted_foreground,
                ))
                .child(div().text_base().font_medium().child("开始使用 MQTTX"))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .text_center()
                        .child("从左侧选择一个连接，或新建连接开始调试"),
                )
                .child(
                    Button::new("empty-new")
                        .icon(IconName::Plus)
                        .label("新建连接")
                        .outline()
                        .small()
                        .mt_1()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_connection_form(None, window, cx);
                        })),
                )
                .into_any_element();
        }

        let active = self.active_tab.clone().unwrap_or_default();
        let mut bar = TabBar::new("conn-tabs")
            // 连接名过长时截断省略，避免单个页签占满整行
            .max_width(px(180.));
        for id in self.open_tabs.clone() {
            let name = self
                .connections
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| id.clone());
            let tab_id = id.clone();
            let close_id = id.clone();
            let is_active = id == active;
            bar = bar.child(
                Tab::new()
                    .label(SharedString::from(name))
                    .selected(is_active)
                    .suffix(
                        Button::new(SharedString::from(format!("tab-close-{}", id)))
                            .icon(IconName::Close)
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.close_tab(&close_id, cx);
                            })),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_tab(&tab_id, window, cx);
                    })),
            );
        }

        let view = self.views.get(&active).cloned();
        // h_full：父级 h_flex 交叉轴居中子项，不占满高度会被整体垂直居中；
        // 视图根是 size_full，须包在 flex_1 容器里，否则会盖住 TabBar 并溢出。
        v_flex()
            .flex_1()
            .h_full()
            .min_h(px(0.))
            .min_w(px(0.))
            .child(div().flex_shrink_0().px_2().pt_1().child(bar))
            .when_some(view, |this, v| {
                this.child(v_flex().flex_1().min_h(px(0.)).child(v))
            })
            .into_any_element()
    }
}
