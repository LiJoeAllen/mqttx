//! 可搜索的枚举下拉：选项模型、委托与统一外观的 Select 包装。

use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::searchable_list::{SearchableListDelegate, SearchableListItem};
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::{Entity, SharedString, Window};

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
    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        Self::Item: SearchableListItem<Value = V>,
        V: PartialEq,
    {
        // 框架契约：按值反查索引，set_selected_value 与过滤视图都依赖它。
        // 返回 None 会导致下拉显示空、保存写默认值，这里必须如实实现。
        self.items
            .iter()
            .position(|i| i.value() == value)
            .map(IndexPath::new)
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

/// 语言切换后重建下拉文案，并把选中项还原到原索引。
///
/// 必须整体做一遍：set_items 只替换数据源，而触发器显示文本取自构造时
/// 缓存的选中项克隆，不重新 set_selected_index 界面就滞留旧语言。
/// C 泛型以兼容 App 与 Context 两种上下文（Entity::update 只要求 AppContext）。
pub fn refresh_select<C: gpui_kit::AppContext>(
    state: &Entity<SelectState<OptionDelegate>>,
    labels: &[&str],
    window: &mut Window,
    cx: &mut C,
) {
    let keep = state
        .read_with(cx, |s, _| s.selected_value().copied().unwrap_or(0))
        .min(labels.len().saturating_sub(1));
    state.update(cx, |s, cx| {
        s.set_items(OptionDelegate::new(labels), window, cx);
        s.set_selected_index(Some(IndexPath::new(keep)), window, cx);
    });
}

/// 语言切换后刷新单行输入框占位文案（占位在构造时固化，需显式重设）。
pub fn refresh_placeholder<C: gpui_kit::AppContext>(
    state: &Entity<InputState>,
    text: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut C,
) {
    state.update(cx, |s, cx| s.set_placeholder(text, window, cx));
}

/// refresh_placeholder 的多行文本域变体。
pub fn refresh_textarea_placeholder<C: gpui_kit::AppContext>(
    state: &Entity<TextareaState>,
    text: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut C,
) {
    state.update(cx, |s, cx| s.set_placeholder(text, window, cx));
}

/// 带可访问名称的下拉包装。
///
/// Select 自身不显示文字标签，直接 `Select::new` 渲染出的 UIA 名称是空的，
/// 屏幕阅读器只报「组合框」而不报它是干什么的。label 由调用方传**已翻译**
/// 文案（`i18n::t("pub.qos")`），这样语言包覆盖率测试能扫到这些 key。
pub fn named_select<D: SearchableListDelegate>(
    state: &Entity<SelectState<D>>,
    label: impl Into<SharedString>,
) -> Select<D>
where
    <D::Item as SearchableListItem>::Value: PartialEq + Clone,
{
    Select::new(state).accessibility_label(label)
}

// ─── 字段行 ──────────────────────────────────────────────────────────────────

/// Select::new 的包装，统一外观。
pub fn select_element<D: SearchableListDelegate>(state: &Entity<SelectState<D>>) -> Select<D>
where
    <D::Item as SearchableListItem>::Value: PartialEq + Clone,
{
    Select::new(state)
}
