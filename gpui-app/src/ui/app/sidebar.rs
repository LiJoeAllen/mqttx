//! 侧边栏：搜索、分组过滤 chips、连接列表与右键菜单。


use gpui_kit::component::button::{ Button, ButtonVariant };
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{ PopupMenuItem };
use gpui_kit::component::{ h_flex, v_flex,  };
use gpui_kit::{ div, px, Context, IntoElement, SharedString, Window,  };

use crate::model::{ ConnectionConfig, ConnectionStatus,  };
use crate::ui::IconName;
use crate::ui::widgets::status_color;

use super::*;
use crate::ui::i18n;

impl MqttXApp {
    /// 侧栏收起后的窄 rail：抽屉把手（拉回侧栏）+ 新建连接入口。
    pub(super) fn render_sidebar_rail(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w(px(40.))
            .h_full()
            .flex_shrink_0()
            .items_center()
            .pt_2()
            .gap_1()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().sidebar_foreground)
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("sidebar-rail-expand")
                    .icon(IconName::PanelLeftOpen)
                    .ghost()
                    .small()
                    .tooltip(i18n::t("展开连接侧栏（Ctrl+B）"))
                    .accessibility_label(i18n::t("展开连接侧栏（Ctrl+B）"))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
            )
            .child(
                Button::new("sidebar-rail-add")
                    .icon(IconName::Plus)
                    .ghost()
                    .small()
                    .tooltip(i18n::t("新建连接"))
                    .accessibility_label(i18n::t("新建连接"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_connection_form(None, window, cx);
                    })),
            )
    }
pub(super) fn render_sidebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {        let query = self.search.read(cx).value().to_lowercase();
        let search = self.search.clone();
        // 分组被改名/删除后过滤器可能悬空（列表恒空），渲染前复位到「全部」
        let dangling = matches!(&self.group_filter, GroupFilter::Named(name)
            if !self.connections.iter().any(|c| {
                c.group.as_deref().map(str::trim) == Some(name.as_str())
            }));
        if dangling {
            self.group_filter = GroupFilter::All;
        }
        let filter = self.group_filter.clone();

        let total = self.connections.len();
        let hover_bg = cx.theme().muted;
        let mut rows = v_flex().gap_0p5().flex_1().overflow_y_scrollbar();
        let mut shown = 0usize;
        for conn in self.connections.clone() {
            // 分组过滤与搜索叠加（AND）
            let group = conn.group.as_deref().map(str::trim).unwrap_or("");
            let match_group = match &filter {
                GroupFilter::All => true,
                GroupFilter::Ungrouped => group.is_empty(),
                GroupFilter::Named(name) => group == name.as_str(),
            };
            if !match_group {
                continue;
            }
            if !query.is_empty() && !conn.name.to_lowercase().contains(&query) {
                continue;
            }
            shown += 1;
            let status = self
                .statuses
                .get(&conn.id)
                .copied()
                .unwrap_or(ConnectionStatus::Disconnected);
            // Error 状态把失败原因直接展示在行内（截断防溢出）：
            // 只有一个红点时用户必须点进 tab 才知道为什么失败
            let error_line = matches!(status, ConnectionStatus::Error)
                .then(|| self.errors.get(&conn.id).cloned())
                .flatten();
            let active = self.active_tab.as_deref() == Some(conn.id.as_str());
            let cfg = conn.clone();
            let name = conn.name.clone();
            let address = conn.display_address();
            let row_id = conn.id.clone();

            rows = rows.child(
                h_flex()
                    .id(SharedString::from(format!("conn-row-{}", conn.id)))
                    .w_full()
                    .items_center()
                    .gap_2()
                    .mx_2()
                    .my_0p5()
                    .px_2()
                    .py_2()
                    .rounded_md()
                    .when(active, |this| this.bg(cx.theme().secondary))
                    .when(!active, |this| {
                        this.hover(move |this| this.bg(hover_bg))
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_tab(&row_id, window, cx);
                    }))
                    .child(
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .bg(status_color(status, cx)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .when(active, |t| t.font_semibold())
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(address),
                            )
                            .when_some(error_line, |v, e| {
                                v.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().danger)
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .child(e),
                                )
                            }),
                    )
                    .child(self.render_conn_power(&cfg, status, cx))
                    .child(self.render_conn_menu(&conn.id, cx)),
            );
        }

        let body = if self.connections.is_empty() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .p_4()
                .child(
                    gpui_kit::component::Icon::new(IconName::PlugZap)
                        .text_color(cx.theme().muted_foreground),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(i18n::t("还没有连接，点击上方 + 新建")),
                )
        } else if shown == 0 {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .p_4()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(i18n::t("没有匹配的连接")),
                )
        } else {
            v_flex().flex_1().child(rows)
        };

        v_flex()
            .w(px(264.))
            .h_full()
            .flex_shrink_0()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().sidebar_foreground)
            // 深色主题下侧边栏与主区同色，补一条分隔线保持边界可辨
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .h_11()
                    .px_3()
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(div().text_sm().font_semibold().child(i18n::t("连接")))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(total.to_string()),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_0p5()
                            .child(
                                Button::new("sidebar-collapse")
                                    .icon(IconName::PanelLeftClose)
                                    .ghost()
                                    .xsmall()
                                    .tooltip(i18n::t("收起侧栏（Ctrl+B）"))
                                    .accessibility_label(i18n::t("收起侧栏（Ctrl+B）"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_sidebar(cx)
                                    })),
                            )
                            .child(
                                Button::new("sidebar-add")
                                    .icon(IconName::Plus)
                                    .tooltip(i18n::t("新建连接"))
                            .accessibility_label(i18n::t("新建连接"))
                                    .ghost()
                                    .xsmall()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_connection_form(None, window, cx);
                                    })),
                            ),
                    ),
            )
            .child(v_flex().px_2().pb_2().child(Input::new(&search).small().prefix(
                gpui_kit::component::Icon::new(IconName::Search).small(),
            )))
            .child(self.render_group_chips(cx))
            .child(body)
    }

    /// 搜索框下方的分组 chips：全部 / 未分组 / 各分组（带计数徽标），点击切换过滤。
    fn render_group_chips(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut names: Vec<String> = self
            .connections
            .iter()
            .filter_map(|c| c.group.as_deref().map(str::trim))
            .filter(|g| !g.is_empty())
            .map(str::to_string)
            .collect();
        names.sort();
        names.dedup();

        let ungrouped = self
            .connections
            .iter()
            .filter(|c| c.group.as_deref().map(str::trim).unwrap_or("").is_empty())
            .count();

        // 分组多时限制高度并纵向滚动，避免 chips 换行挤占下方连接列表
        let mut row = h_flex()
            .gap_1()
            .flex_wrap()
            .px_2()
            .pb_1()
            .max_h(px(88.))
            .overflow_y_scrollbar();
        row = row.child(self.render_group_chip(
            "chip-all",
            format!("{} {}", i18n::t("全部"), self.connections.len()),
            GroupFilter::All,
            cx,
        ));
        row = row.child(self.render_group_chip(
            "chip-ungrouped",
            format!("{} {ungrouped}", i18n::t("未分组")),
            GroupFilter::Ungrouped,
            cx,
        ));
        for name in names {
            let count = self
                .connections
                .iter()
                .filter(|c| c.group.as_deref().map(str::trim) == Some(name.as_str()))
                .count();
            row = row.child(self.render_group_chip(
                SharedString::from(format!("chip-g-{name}")),
                format!("{name} {count}"),
                GroupFilter::Named(name),
                cx,
            ));
        }
        row
    }

    fn render_group_chip(
        &self,
        id: impl Into<gpui_kit::ElementId>,
        label: String,
        filter: GroupFilter,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = match (&self.group_filter, &filter) {
            (GroupFilter::All, GroupFilter::All)
            | (GroupFilter::Ungrouped, GroupFilter::Ungrouped) => true,
            (GroupFilter::Named(a), GroupFilter::Named(b)) => a == b,
            _ => false,
        };
        div()
            .id(id)
            .px_2()
            .py_0p5()
            .rounded_md()
            .text_xs()
            .when(selected, |d| d.bg(cx.theme().secondary).font_semibold())
            .when(!selected, |d| {
                d.bg(cx.theme().muted)
                    .text_color(cx.theme().muted_foreground)
                    .hover(|d| d.bg(cx.theme().secondary))
            })
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                // listener 是 Fn（可多次调用），只能克隆而不能移出捕获的 filter
                this.group_filter = filter.clone();
                cx.notify();
            }))
            .child(label)
    }

    fn render_conn_power(
        &self,
        cfg: &ConnectionConfig,
        status: ConnectionStatus,
        cx: &mut Context<Self>,
    ) -> Button {
        let cfg = cfg.clone();
        let connected = matches!(status, ConnectionStatus::Connected);
        Button::new(SharedString::from(format!("conn-power-{}", cfg.id)))
            .icon(if connected { IconName::Square } else { IconName::PlugZap })
            .ghost()
            .xsmall()
            .tooltip(if connected { i18n::t("断开") } else { i18n::t("连接") })
            .accessibility_label(if connected { i18n::t("断开") } else { i18n::t("连接") })
            .on_click(cx.listener(move |this, _, _, _| {
                this.toggle_connection(&cfg);
            }))
    }

    fn render_conn_menu(&self, conn_id: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let eid = conn_id.to_string();
        Button::new(SharedString::from(format!("conn-menu-{}", conn_id)))
            .icon(IconName::Ellipsis)
            .ghost()
            .xsmall()
            .dropdown_menu(move |menu, _, _| {
                let (e1, e2, e3) = (eid.clone(), eid.clone(), eid.clone());
                let (w1, w2, w3) = (weak.clone(), weak.clone(), weak.clone());
                menu.item(PopupMenuItem::new(i18n::t("编辑")).on_click(move |_, window, cx| {
                    w1.update(cx, |app, cx| {
                        let cfg = app.connections.iter().find(|c| c.id == e1).cloned();
                        if let Some(cfg) = cfg {
                            app.open_connection_form(Some(cfg), window, cx);
                        }
                    })
                    .ok();
                }))
                .item(PopupMenuItem::new(i18n::t("复制连接")).on_click(move |_, _, cx| {
                    w2.update(cx, |app, cx| {
                        app.duplicate_connection(&e2);
                        cx.notify();
                    })
                    .ok();
                }))
                .separator()
                .item(PopupMenuItem::new(i18n::t("删除")).on_click(move |_, window, cx| {
                    let Some(app) = w3.upgrade() else {
                        return;
                    };
                    // 二次确认：删除会级联清理订阅并断开连接，不可撤销
                    let (name, sub_count) = {
                        let a = app.read(cx);
                        let name = a
                            .connections
                            .iter()
                            .find(|c| c.id == e3)
                            .map(|c| c.name.clone())
                            .unwrap_or_else(|| e3.clone());
                        let n = a.subscriptions.iter().filter(|s| s.connection_id == e3).count();
                        (name, n)
                    };
                    let confirm_id = e3.clone();
                    window.open_alert_dialog(cx, move |alert, _, _| {
                        // 构建闭包是 Fn（可重复调用），克隆后再移入按钮回调
                        let app = app.clone();
                        let confirm_id = confirm_id.clone();
                        alert
                            .title(i18n::t("删除连接"))
                            .description(SharedString::from(i18n::tf(
                                "确定删除「{name}」？将同时删除 {sub_count} 个订阅，并断开当前连接，此操作不可撤销。",
                                &[("name", &name), ("sub_count", &sub_count.to_string())],
                            )))
                            .button_props(
                                DialogButtonProps::default()
                                    .ok_text(i18n::t("删除"))
                                    .ok_variant(ButtonVariant::Danger)
                                    .show_cancel(true),
                            )
                            .on_ok(move |_, _, cx| {
                                app.update(cx, |app, cx| app.delete_connection(&confirm_id, cx));
                                true
                            })
                    });
                }))
            })
    }
}
