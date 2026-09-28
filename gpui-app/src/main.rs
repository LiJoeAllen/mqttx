use gpui_kit::component::{Root, TitleBar};
use gpui_kit::*;

use mqttx_desktop::ui;
use mqttx_desktop::update;

fn main() {
    // OTA 收尾：清理遗留文件；若暂存区有待安装的新版本（用户上次同意安装），
    // 在进入 UI 前完成自替换并重启。
    update::cleanup_old();
    if let Some(staged) = update::load_staged()
        && update::install_staged(&staged).is_ok()
    {
        // 新进程已拉起，当前进程立即退出
        std::process::exit(0);
    }

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
