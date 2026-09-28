use gpui_kit::component::{Root, TitleBar};
use gpui_kit::*;

use mqttx_desktop::ui;
use mqttx_desktop::update;

/// 自建 Sentry（rustrak）：panic 与关键错误上报。
const SENTRY_DSN: &str =
    "https://9853b05c2a3a4fce921c650587168f49@api.rustrak.sentry.heavenlybook.cn/8";

/// 返回的 guard 必须在程序整个生命周期内存活，drop 即停用上报。
fn init_sentry() -> sentry::ClientInitGuard {
    sentry::init(
        sentry::ClientOptions::new()
            .dsn(SENTRY_DSN)
            .release(format!("mqttx@{}", env!("CARGO_PKG_VERSION"))),
    )
}

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

    let _sentry_guard = init_sentry();
    // sentry 的 panic 钩子只捕获不等待传输；再包一层在钩子末尾 flush，
    // 保证 panic=abort 立即终止进程前事件已送出。
    let chained_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        chained_panic_hook(info);
        if let Some(client) = sentry::Hub::current().client() {
            client.flush(Some(std::time::Duration::from_secs(5)));
        }
    }));

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
