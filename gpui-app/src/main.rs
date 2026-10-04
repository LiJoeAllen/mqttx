use gpui_kit::component::TitleBar;
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
            // 与更新检查用同一版本来源（尊重 MQTTX_VERSION_OVERRIDE），保持一致
            .release(format!("mqttx@{}", update::current_version())),
    )
}

fn main() {
    // 资源监控的运行时长基准
    mqttx_desktop::sysmon::init();
    // OTA 收尾：清理遗留文件；若暂存区有待安装的新版本且用户已同意
    // （「下次启动安装」），在进入 UI 前完成自替换并重启。
    update::cleanup_old();
    if let Some(staged) = update::load_staged() {
        if update::install_consent_matches(&staged) {
            match update::install_staged(&staged) {
                Ok(()) => {
                    // 新进程已拉起，当前进程立即退出
                    std::process::exit(0);
                }
                Err(_) => {
                    // 安装失败：清除同意标记，继续正常启动，
                    // 由应用内的更新检查重新弹窗询问。
                    update::clear_install_consent();
                }
            }
        } else {
            // 暂存包存在但用户未同意安装（例如跳过了确认对话框）：
            // 不做任何替换，等待应用内更新流程再次询问。
        }
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
                // gpui-kit 组件内置文案（按钮/日历/空态等）并入当前语言包
            mqttx_desktop::init_i18n();
            gpui_kit::init(cx);

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1440.), px(900.)),
                    cx,
                ))),
                ..TitleBar::window_options()
            };

            // 0.7 起 gpui_kit::open_window 自动挂载 Root（承载 dialog/sheet/notification 层）
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| ui::app::MqttXApp::new(window, cx))
            })
            .expect("Failed to open window");
        });
}
