//! 标题栏：窗口控制、钉子置顶、主题切换与全局动作菜单。


use gpui_kit::component::button::{ Button };
use gpui_kit::component::menu::{ PopupMenuItem };
use gpui_kit::component::{ h_flex, notification::Notification, ThemeMode, TitleBar,  };
use gpui_kit::{ div, px, Context, IntoElement, Window,  };

use crate::model::ThemeModePref;
use crate::ui::IconName;

use super::*;

impl MqttXApp {
pub(super) fn render_titlebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app_weak = cx.entity().downgrade();

        TitleBar::new().child(
            h_flex()
                .w_full()
                .h_full()
                .items_center()
                .justify_between()
                .pl_2()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(crate::ui::logo::logo(
                            px(26.),
                            cx.theme().primary,
                            cx.theme().primary_foreground,
                        ))
                        .child(div().text_sm().font_semibold().child("MQTTX"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("MQTT 调试客户端"),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .pr_2()
                        .items_center()
                        // 整条标题栏被 gpui-component 标记为 WindowControlArea::Drag
                        // （WM_NCHITTEST 返回 HTCAPTION），点击会变成拖动窗口。
                        // 按钮簇必须遮挡鼠标：hit_test 收集到 BlockMouse hitbox
                        // 即截断，父级 Drag 区不再命中，按钮才能收到点击。
                        .occlude()
                        .child(
                            Button::new("new-connection")
                                .icon(IconName::Plus)
                                .label("新建连接")
                                .primary()
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_connection_form(None, window, cx);
                                })),
                        )
                        .child(
                            Button::new("pin-window")
                                .icon(if self.pinned {
                                    IconName::PinOff
                                } else {
                                    IconName::Pin
                                })
                                .ghost()
                                .small()
                                .tooltip(if self.pinned {
                                    "取消窗口置顶"
                                } else {
                                    "窗口置顶"
                                })
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let next = !this.pinned;
                                    if crate::platform::set_topmost(window, next) {
                                        this.pinned = next;
                                        cx.notify();
                                    } else {
                                        window.push_notification(
                                            Notification::error("置顶操作失败"),
                                            cx,
                                        );
                                    }
                                })),
                        )
                        .child(self.render_theme_toggle(cx))
                        .child(
                            Button::new("more-menu")
                                .icon(IconName::Ellipsis)
                                .ghost()
                                .small()
                                .dropdown_menu(move |menu, _, _| {
                                    let w1 = app_weak.clone();
                                    let w2 = app_weak.clone();
                                    let w3 = app_weak.clone();
                                    let w4 = app_weak.clone();
                                    let w5 = app_weak.clone();
                                    let w6 = app_weak.clone();
                                    menu.item(
                                        PopupMenuItem::new("阿里云设备")
                                            .on_click(move |_, window, cx| {
                                                if let Some(entity) = w1.upgrade() {
                                                    crate::ui::dialogs::aliyun_dialog::open(entity, window, cx);
                                                }
                                            }),
                                    )
                                    .item(PopupMenuItem::new("全局变量").on_click(
                                        move |_, window, cx| {
                                            if let Some(entity) = w2.upgrade() {
                                                crate::ui::dialogs::variables_dialog::open(entity, window, cx);
                                            }
                                        },
                                    ))
                                    .item(PopupMenuItem::new("资源监控").on_click(
                                        move |_, window, cx| {
                                            if let Some(entity) = w6.upgrade() {
                                                crate::ui::dialogs::resource_dialog::open(entity, window, cx);
                                            }
                                        },
                                    ))
                                    .separator()
                                    .item(PopupMenuItem::new("导出连接").on_click(
                                        move |_, window, cx| {
                                            if let Some(entity) = w4.upgrade() {
                                                entity.update(cx, |app, cx| {
                                                    app.export_connections(window, cx)
                                                });
                                            }
                                        },
                                    ))
                                    .item(PopupMenuItem::new("导入连接").on_click(
                                        move |_, window, cx| {
                                            if let Some(entity) = w5.upgrade() {
                                                entity.update(cx, |app, cx| {
                                                    app.import_connections(window, cx)
                                                });
                                            }
                                        },
                                    ))
                                    .separator()
                                    .item(PopupMenuItem::new("设置").on_click(
                                        move |_, window, cx| {
                                            if let Some(entity) = w3.upgrade() {
                                                crate::ui::dialogs::settings_dialog::open(entity, window, cx);
                                            }
                                        },
                                    ))
                                }),
                        ),
                ),
        )
    }

    fn render_theme_toggle(&self, cx: &mut Context<Self>) -> Button {
        let dark = cx.theme().mode == ThemeMode::Dark;
        Button::new("toggle-theme")
            .icon(if dark { IconName::Sun } else { IconName::Moon })
            .ghost()
            .small()
            .tooltip(if dark { "切换为浅色" } else { "切换为深色" })
            .on_click(cx.listener(|this, _, _, cx| {
                let next = if cx.theme().mode == ThemeMode::Dark {
                    ThemeModePref::Light
                } else {
                    ThemeModePref::Dark
                };
                let mut s = this.settings.clone();
                s.theme = next;
                this.save_settings(s, cx);
                cx.notify();
            }))
    }
}
