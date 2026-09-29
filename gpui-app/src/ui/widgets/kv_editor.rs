//! key/value 列表编辑器（MQTT 5 用户属性等）。

use gpui_kit::component::button::{ Button, ButtonVariants as _ };
use gpui_kit::component::input::{ Input, InputState };
use gpui_kit::component::{ h_flex, v_flex, Disableable as _, Sizable as _ };
use crate::ui::IconName;
use gpui_kit::{ App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div, px };


/// MQTT 5 用户属性等 key/value 列表编辑器。
pub struct KvEditor {
    id_prefix: SharedString,
    pub rows: Vec<(Entity<InputState>, Entity<InputState>)>,
}

impl KvEditor {
    pub fn new(
        id_prefix: impl Into<SharedString>,
        pairs: &[(String, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut editor = Self {
            id_prefix: id_prefix.into(),
            rows: Vec::new(),
        };
        for (k, v) in pairs {
            editor.push(Some((k.as_str(), v.as_str())), window, cx);
        }
        if editor.rows.is_empty() {
            editor.push(None, window, cx);
        }
        editor
    }

    fn push(&mut self, initial: Option<(&str, &str)>, window: &mut Window, cx: &mut Context<Self>) {
        let key = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("key");
            if let Some((k, _)) = initial {
                s.set_value(k, window, cx);
            }
            s
        });
        let value = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("value");
            if let Some((_, v)) = initial {
                s.set_value(v, window, cx);
            }
            s
        });
        self.rows.push((key, value));
    }

    pub fn pairs(&self, cx: &App) -> Vec<(String, String)> {
        // key 统一 trim 并去重（保留最后一条）：否则带首尾空白的 key
        // 永远无法被 {{key}} 模板引用，重复 key 也只有一条能生效。
        let mut out: Vec<(String, String)> = Vec::new();
        for (k, v) in self
            .rows
            .iter()
            .map(|(k, v)| (k.read(cx).value().to_string(), v.read(cx).value().to_string()))
        {
            let k = k.trim().to_string();
            if k.is_empty() {
                continue;
            }
            match out.iter_mut().find(|(ek, _)| *ek == k) {
                Some(slot) => slot.1 = v,
                None => out.push((k, v)),
            }
        }
        out
    }
}

impl Render for KvEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let prefix = self.id_prefix.clone();
        let count = self.rows.len();

        let mut rows = v_flex().gap_1();
        for (i, (key, value)) in self.rows.iter().cloned().enumerate() {
            rows = rows.child(
                h_flex()
                    .id(SharedString::from(format!("kv-row-{prefix}-{i}")))
                    .gap_1()
                    .child(div().flex_1().min_w(px(0.)).child(Input::new(&key)))
                    .child(div().flex_1().min_w(px(0.)).child(Input::new(&value)))
                    .child(
                        Button::new(SharedString::from(format!("kv-del-{prefix}-{i}")))
                            .icon(IconName::Close)
                            .ghost()
                            .xsmall()
                            .disabled(count <= 1)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.rows.len() > 1 {
                                    this.rows.remove(i);
                                    cx.notify();
                                }
                            })),
                    ),
            );
        }

        v_flex()
            .gap_2()
            .child(rows)
            .child(
                Button::new(SharedString::from(format!("kv-add-{prefix}")))
                    .icon(IconName::Plus)
                    .label("添加属性")
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.push(None, window, cx);
                        cx.notify();
                    })),
            )
    }
}

// ─── 时间格式化 ──────────────────────────────────────────────────────────────
