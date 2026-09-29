//! 用户动作：连接/订阅/预设/变量/设置等状态写操作，与连接表单的打开。


use gpui_kit::component::button::{ Button };
use gpui_kit::component::{ h_flex, notification::Notification,  };
use gpui_kit::{ div, px, App, Context, Window,  };

use crate::aliyun::AliyunPreset;
use crate::model::{ AppSettings, ConnectionConfig, ConnectionStatus, GlobalVariable, PublishParams, PublishPreset, Subscription,  };
use crate::ui::IconName;
use crate::ui::{ connection_form::ConnectionForm, connection_view::ConnectionView,  };

use super::*;

impl MqttXApp {
    pub fn save_connection(&mut self, cfg: ConnectionConfig) {
        if let Some(slot) = self.connections.iter_mut().find(|c| c.id == cfg.id) {
            *slot = cfg;
        } else {
            self.connections.push(cfg);
        }
        self.storage.save_connections(&self.connections);
    }

    pub fn duplicate_connection(&mut self, id: &str) {
        let Some(src) = self.connections.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        let mut copy = src;
        copy.id = uuid::Uuid::new_v4().to_string();
        copy.name = format!("{} 副本", copy.name);
        copy.client_id =
            format!("mqttx_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        self.connections.push(copy);
        self.storage.save_connections(&self.connections);
    }

    pub fn delete_connection(&mut self, id: &str, cx: &mut Context<Self>) {
        self.engine.close(id, false);
        self.connections.retain(|c| c.id != id);
        self.subscriptions.retain(|s| s.connection_id != id);
        self.clear_messages(id);
        self.statuses.remove(id);
        self.errors.remove(id);
        self.views.remove(id);
        self.open_tabs.retain(|t| t != id);
        if self.active_tab.as_deref() == Some(id) {
            self.active_tab = self.open_tabs.last().cloned();
        }
        self.storage.save_connections(&self.connections);
        self.storage.save_subscriptions(&self.subscriptions);
        cx.notify();
    }

    pub fn toggle_connection(&self, cfg: &ConnectionConfig) {
        // Connecting 期间 is_connected 恒为 false：防抖，双击/误触不会
        // 并发两次 connect（会互相顶掉句柄并在 broker 侧形成重连风暴）
        if self.statuses.get(&cfg.id) == Some(&ConnectionStatus::Connecting) {
            return;
        }
        if self.engine.is_connected(&cfg.id) {
            self.engine.close(&cfg.id, true);
        } else {
            // 遗嘱支持 {{变量}}：对副本注入，保存的配置仍为模板原文
            let mut cfg = cfg.clone();
            crate::model::render_will_templates(&mut cfg, &self.variables);
            self.engine.connect(cfg);
        }
    }

    /// 打开的标签数（资源监控展示用）。
    pub fn open_tab_count(&self) -> usize {
        self.open_tabs.len()
    }

    pub fn open_tab(&mut self, conn_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open_tabs.contains(&conn_id.to_string()) {
            self.open_tabs.push(conn_id.to_string());
            let weak = cx.entity().downgrade();
            let engine = self.engine.clone();
            let view = cx.new(|cx| ConnectionView::new(conn_id.to_string(), weak, engine, window, cx));
            self.views.insert(conn_id.to_string(), view);
        }
        self.active_tab = Some(conn_id.to_string());
        cx.notify();
    }

pub(super) fn close_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        self.open_tabs.retain(|t| t != id);
        self.views.remove(id);
        if self.active_tab.as_deref() == Some(id) {
            self.active_tab = self.open_tabs.last().cloned();
        }
        cx.notify();
    }

    // ── 订阅 ──────────────────────────────────────────────────────────────

    pub fn add_subscription(&mut self, sub: Subscription) {
        if !self
            .subscriptions
            .iter()
            .any(|s| s.connection_id == sub.connection_id && s.topic == sub.topic)
        {
            self.subscriptions.push(sub);
            self.storage.save_subscriptions(&self.subscriptions);
        }
    }

    pub fn remove_subscription(&mut self, connection_id: &str, topic: &str) {
        self.engine
            .unsubscribe(connection_id.to_string(), topic.to_string());
        self.subscriptions
            .retain(|s| !(s.connection_id == connection_id && s.topic == topic));
        self.storage.save_subscriptions(&self.subscriptions);
    }

    // ── 变量 / 预设 / 阿里云 / 设置 ──────────────────────────────────────────

    pub fn save_variables(&mut self, vars: Vec<GlobalVariable>) {
        self.variables = vars;
        self.storage.save_variables(&self.variables);
    }

    pub fn save_presets(&mut self, presets: Vec<PublishPreset>) {
        self.presets = presets;
        self.storage.save_presets(&self.presets);
    }

    /// 保存一个发布预设（按 name 去重更新）。
    pub fn upsert_publish_preset(&mut self, name: String, params: PublishParams) {
        match self.presets.iter_mut().find(|p| p.name == name) {
            Some(p) => p.params = params,
            None => self.presets.push(PublishPreset::new(name, params)),
        }
        self.storage.save_presets(&self.presets);
    }

    pub fn delete_publish_preset(&mut self, id: &str) {
        self.presets.retain(|p| p.id != id);
        self.storage.save_presets(&self.presets);
    }

    pub fn save_aliyun(&mut self, presets: Vec<AliyunPreset>) {
        self.aliyun_presets = presets;
        self.storage.save_aliyun(&self.aliyun_presets);
    }

    pub fn save_settings(&mut self, settings: AppSettings, cx: &mut App) {
        apply_theme(settings.theme, cx);
        self.settings = settings;
        self.storage.save_settings(&self.settings);
    }


    pub fn open_connection_form(
        &mut self,
        edit: Option<ConnectionConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.entity().downgrade();
        let engine = self.engine.clone();
        // 表单的分组下拉需要现有分组列表（去重、排序）
        let mut groups: Vec<String> = self
            .connections
            .iter()
            .filter_map(|c| c.group.as_ref().map(|g| g.trim().to_string()))
            .filter(|g| !g.is_empty())
            .collect();
        groups.sort();
        groups.dedup();
        let form = cx.new(|cx| ConnectionForm::new(engine, edit, groups, window, cx));
        window.open_dialog(cx, move |dialog, _, cx| {
            let form_ok = form.clone();
            let form_test = form.clone();
            let app_ok = app.clone();
            let testing = form_test.read(cx).is_testing();
            dialog
                .w(px(680.))
                .title("连接配置")
                .child(form.clone())
                .footer(
                    h_flex()
                        .gap_2()
                        .w_full()
                        // 测试连接常驻左下角，保存前即可验证配置
                        .child(
                            Button::new("form-test")
                                .icon(if testing {
                                    IconName::LoaderCircle
                                } else {
                                    IconName::PlugZap
                                })
                                .label(if testing { "测试中…" } else { "测试连接" })
                                .outline()
                                .loading(testing)
                                .on_click(move |_, window, cx| {
                                    form_test.update(cx, |f, cx| f.run_test(window, cx));
                                }),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("form-cancel")
                                .label("取消")
                                .outline()
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("form-ok")
                                .label("保存")
                                .primary()
                                .on_click(move |_, window, cx| {
                                    let built = form_ok.read(cx).build(cx);
                                    match built {
                                        Ok(cfg) => {
                                            // 已连接的连接改了关键参数，重连后才生效，先算好再保存
                                            let mut stale = false;
                                            app_ok
                                                .update(cx, |app, cx| {
                                                    let connected = app.statuses.get(&cfg.id)
                                                        == Some(&ConnectionStatus::Connected);
                                                    if connected
                                                        && app
                                                            .connections
                                                            .iter()
                                                            .find(|c| c.id == cfg.id)
                                                            .is_some_and(|old| {
                                                                old.session_params_changed(&cfg)
                                                            })
                                                    {
                                                        stale = true;
                                                    }
                                                    app.save_connection(cfg);
                                                    cx.notify();
                                                })
                                                .ok();
                                            window.close_dialog(cx);
                                            if stale {
                                                window.push_notification(
                                                    Notification::info("配置已保存，重连后生效"),
                                                    cx,
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            // 触发重绘，让字段级红字即时显示（错误已写入表单内部）
                                            form_ok.update(cx, |_, cx| cx.notify());
                                            window.push_notification(
                                                Notification::error(e),
                                                cx,
                                            );
                                        }
                                    }
                                }),
                        ),
                )
        });
    }
}
