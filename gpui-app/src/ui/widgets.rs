//! 跨界面复用的小组件：枚举下拉、键值对编辑器、状态指示、字段行等。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::searchable_list::{SearchableListDelegate, SearchableListItem};
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Disableable as _, IndexPath, Sizable as _};
use crate::ui::IconName;
use gpui_kit::{App, AppContext as _, Context, Entity, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div, px};

use crate::model::ConnectionStatus;

// ─── 枚举下拉 ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct OptionItem {
    pub value: usize,
    pub label: SharedString,
}

impl SearchableListItem for OptionItem {
    type Value = usize;
    fn title(&self) -> SharedString {
        self.label.clone()
    }
    fn value(&self) -> &usize {
        &self.value
    }
}

#[derive(Clone)]
pub struct OptionDelegate {
    pub items: Vec<OptionItem>,
}

impl OptionDelegate {
    pub fn new(labels: &[&str]) -> Self {
        Self {
            items: labels
                .iter()
                .enumerate()
                .map(|(i, l)| OptionItem {
                    value: i,
                    label: (*l).into(),
                })
                .collect(),
        }
    }
}

impl SearchableListDelegate for OptionDelegate {
    type Item = OptionItem;
    fn items_count(&self, _section: usize) -> usize {
        self.items.len()
    }
    fn item(&self, ix: IndexPath) -> Option<&Self::Item> {
        self.items.get(ix.row)
    }
    fn position<V>(&self, _value: &V) -> Option<IndexPath>
    where
        Self::Item: SearchableListItem<Value = V>,
        V: PartialEq,
    {
        // 选中项统一通过 IndexPath 构造，无需按值反查
        None
    }
}

/// 创建字符串枚举下拉状态（在任意实体上下文里调用）。
pub fn make_select<C: gpui_kit::AppContext>(
    labels: &[&str],
    selected: usize,
    window: &mut Window,
    cx: &mut C,
) -> Entity<SelectState<OptionDelegate>> {
    let selected = selected.min(labels.len().saturating_sub(1));
    cx.new(|cx| {
        SelectState::new(
            OptionDelegate::new(labels),
            Some(IndexPath::new(selected)),
            window,
            cx,
        )
    })
}

// ─── 字段行 ──────────────────────────────────────────────────────────────────

/// 小号标签 + 控件的垂直字段容器（可选字段，无校验态）。
pub fn field(label: impl Into<SharedString>, control: impl IntoElement) -> impl IntoElement {
    field_ex(label, control, false, None, Hsla::default())
}

/// 字段行变体：`required` 时标签后追加红色 `*`；
/// `error` 为 Some 时在控件下方渲染红字提示（`danger` 为主题错误色）。
pub fn field_ex(
    label: impl Into<SharedString>,
    control: impl IntoElement,
    required: bool,
    error: Option<SharedString>,
    danger: Hsla,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .min_w(px(0.))
        .child(
            h_flex()
                .gap_0p5()
                .items_baseline()
                .child(div().text_xs().child(label.into()))
                .children(required.then(|| div().text_xs().text_color(danger).child("*"))),
        )
        .child(control)
        .children(error.map(|e| div().text_xs().text_color(danger).child(e)))
}

// ─── 状态颜色 ────────────────────────────────────────────────────────────────

pub fn status_color(status: ConnectionStatus, cx: &App) -> Hsla {
    let t = cx.theme();
    match status {
        ConnectionStatus::Connected => t.success,
        ConnectionStatus::Connecting => t.warning,
        ConnectionStatus::Error => t.danger,
        ConnectionStatus::Disconnected => t.muted_foreground,
    }
}

pub fn status_dot(status: ConnectionStatus, cx: &App) -> impl IntoElement {
    div()
        .size(px(8.))
        .rounded_full()
        .bg(status_color(status, cx))
}

// ─── 键值对编辑器 ────────────────────────────────────────────────────────────

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
        self.rows
            .iter()
            .map(|(k, v)| (k.read(cx).value().to_string(), v.read(cx).value().to_string()))
            .filter(|(k, _)| !k.trim().is_empty())
            .collect()
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

pub fn format_time(ts_ms: i64, show_millis: bool) -> String {
    use chrono::TimeZone as _;
    let Some(dt) = chrono::Local.timestamp_millis_opt(ts_ms).single() else {
        return String::new();
    };
    if show_millis {
        dt.format("%H:%M:%S%.3f").to_string()
    } else {
        dt.format("%H:%M:%S").to_string()
    }
}

/// Select::new 的包装，统一外观。
pub fn select_element<D: SearchableListDelegate>(
    state: &Entity<SelectState<D>>,
) -> Select<D>
where
    <D::Item as SearchableListItem>::Value: PartialEq + Clone,
{
    Select::new(state)
}
