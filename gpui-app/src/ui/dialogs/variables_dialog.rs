//! 全局变量管理对话框。变量可在发布主题/负载中以 `{{key}}` 引用。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, v_flex, WindowExt as _};
use gpui_kit::{
    px, App, AppContext as _, Entity, ParentElement as _, Styled as _, Window,
};

use crate::model::GlobalVariable;
use crate::ui::app::MqttXApp;
use crate::ui::i18n;
use crate::ui::widgets::KvEditor;

pub fn open(app: Entity<MqttXApp>, window: &mut Window, cx: &mut App) {
    // 对话框是栈式叠加：标题栏菜单、快捷键、行内按钮等多个入口都会调 open，
    // 不加守卫会压入第二个模态，同 id 的控件在两层之间串台
    if window.has_active_dialog(cx) {
        return;
    }
    let pairs: Vec<(String, String)> = app
        .read(cx)
        .variables
        .iter()
        .map(|v| (v.key.clone(), v.value.clone()))
        .collect();
    let editor: Entity<KvEditor> =
        cx.new(|cx| KvEditor::new("global-var", &pairs, window, cx));
    let app = app.clone();

    window.open_dialog(cx, move |dialog, _, _| {
        let editor_body = editor.clone();
        let for_ok = editor.clone();
        let app_ok = app.clone();
        dialog
            .w(px(560.))
            .title(i18n::t("titlebar.variables"))
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        v_flex()
                            .text_sm()
                            .gap_1()
                            .child(i18n::t("var.dialog_hint"))
                            .child(i18n::t("var.dialog_builtin")),
                    )
                    .child(editor_body),
            )
            .footer(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .w_full()
                    .child(
                        Button::new("vars-cancel")
                            .label(i18n::t("common.cancel"))
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("vars-ok")
                            .label(i18n::t("common.save"))
                            .primary()
                            .on_click(move |_, window, cx| {
                                let vars = for_ok
                                    .read(cx)
                                    .pairs(cx)
                                    .into_iter()
                                    .map(|(key, value)| GlobalVariable { key, value })
                                    .collect();
                                app_ok.update(cx, |a, cx| {
                                    a.save_variables(vars);
                                    cx.notify();
                                });
                                window.close_dialog(cx);
                            }),
                    ),
            )
    });
}
