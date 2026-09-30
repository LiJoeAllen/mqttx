//! 可搜索的枚举下拉：选项模型、委托与统一外观的 Select 包装。

use gpui_kit::component::searchable_list::{ SearchableListDelegate, SearchableListItem };
use gpui_kit::component::select::{ Select, SelectState };
use gpui_kit::component::IndexPath;
use gpui_kit::{ Entity, SharedString, Window };

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

// ─── 字段行 ──────────────────────────────────────────────────────────────────


/// Select::new 的包装，统一外观。
pub fn select_element<D: SearchableListDelegate>(
    state: &Entity<SelectState<D>>,
) -> Select<D>
where
    <D::Item as SearchableListItem>::Value: PartialEq + Clone,
{
    Select::new(state)
}
