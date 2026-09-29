//! 构建脚本：向 exe 嵌入窗口图标资源。
//!
//! gpui-pre-windows 注册窗口类时从 exe 资源 ID 1 加载图标
//! （`LoadImageW(module, MAKEINTRESOURCE(1), IMAGE_ICON, ...)`），
//! 缺失时任务栏与资源管理器显示系统默认图标。

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=resources/app.rc");
        println!("cargo:rerun-if-changed=resources/icon.ico");
        embed_resource::compile("resources/app.rc", embed_resource::NONE)
            .manifest_required()
            .expect("嵌入窗口图标资源失败");
    }
}
