use gpui_kit::component::{Root, TitleBar};
use gpui_kit::*;

use mqttx_desktop::ui;

fn main() {
    application()
        // AllAssets 嵌入全量 Lucide 图标；Assets 只含 default-icons.txt 的 101 个，
        // 其余 IconName（MailOpen/SlidersHorizontal 等）运行时会渲染空白
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1440.), px(900.)),
                    cx,
                ))),
                ..TitleBar::window_options()
            };

            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| ui::app::MqttXApp::new(window, cx));
                cx.new(|cx| Root::new(view.clone(), window, cx))
            })
            .expect("Failed to open window");
        });
}
