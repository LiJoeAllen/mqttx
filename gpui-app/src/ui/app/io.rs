//! 连接配置导入/导出（文件对话框 + JSON 序列化）。

use std::path::PathBuf;

use gpui_kit::component::{ notification::Notification,  };
use gpui_kit::{ Context, Window,  };


use super::*;
use crate::ui::i18n;

impl MqttXApp {
    /// 数据目录（设置对话框展示 / 打开用）。
    pub fn data_dir(&self) -> PathBuf {
        self.storage.dir().to_path_buf()
    }

    /// 日志目录（设置对话框展示 / 打开用）。
    pub fn log_dir(&self) -> PathBuf {
        self.storage.log_dir()
    }

    // ── 连接导入 / 导出 ────────────────────────────────────────────────────

    /// 在后台线程弹出阻塞式原生文件对话框，完成后回主线程执行 `done`。
    /// 对话框进行中置位，防止重复弹出。
    fn spawn_file_dialog<R, F, D>(
        &mut self,
        work: F,
        done: D,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
        D: FnOnce(&mut Self, R, &mut Window, &mut Context<Self>) + 'static,
    {
        if self.file_dialog_open {
            return;
        }
        self.file_dialog_open = true;
        // rfd 的阻塞对话框不可占用 UI 线程：结果经 smol 通道送回异步任务
        let (tx, rx) = smol::channel::bounded::<R>(1);
        std::thread::spawn(move || {
            let _ = tx.send_blocking(work());
        });
        let weak = cx.entity().downgrade();
        cx.spawn_in(window, async move |_this, cx: &mut gpui_kit::AsyncWindowContext| {
            let result = rx.recv().await.ok();
            weak.update_in(cx, |app, window, cx| {
                app.file_dialog_open = false;
                if let Some(result) = result {
                    done(app, result, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// 导出全部连接为 JSON（原生保存对话框，默认文件名 connections-export.json）。
    pub fn export_connections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.storage.dir().to_path_buf();
        self.spawn_file_dialog(
            move || {
                rfd::FileDialog::new()
                    .set_title("导出连接")
                    .set_directory(dir)
                    .set_file_name("connections-export.json")
                    .add_filter("JSON", &["json"])
                    .save_file()
            },
            |app, path, window, cx| {
                let Some(path) = path else {
                    return; // 用户取消
                };
                let count = app.connections.len();
                match crate::store::export_connections_to(&path, &app.connections) {
                    Ok(()) => window.push_notification(
                        Notification::success(i18n::tf(
                            "导出 {n} 条连接到 {path}",
                            &[
                                ("n", &count.to_string()),
                                ("path", &path.display().to_string()),
                            ],
                        )),
                        cx,
                    ),
                    Err(e) => window.push_notification(Notification::error(e), cx),
                }
            },
            window,
            cx,
        );
    }

    /// 导入连接：按 id 去重（同 id 跳过），新 id 保留文件原值；读取解析在后台线程完成。
    pub fn import_connections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.storage.dir().to_path_buf();
        self.spawn_file_dialog(
            move || {
                let Some(path) = rfd::FileDialog::new()
                    .set_title("导入连接")
                    .set_directory(dir)
                    .add_filter("JSON", &["json"])
                    .pick_file()
                else {
                    return Ok(None); // 用户取消
                };
                crate::store::import_connections_from(&path).map(Some)
            },
            |app, result, window, cx| match result {
                Err(e) => {
                    window.push_notification(Notification::error(e), cx);
                }
                Ok(None) => {}
                Ok(Some(items)) => {
                    let mut existing: std::collections::HashSet<String> =
                        app.connections.iter().map(|c| c.id.clone()).collect();
                    let mut added = 0usize;
                    let mut skipped = 0usize;
                    for item in items {
                        if existing.contains(&item.id) {
                            skipped += 1;
                        } else {
                            // 记录已入库 id，导入文件内部的重复 id 也只收一条
                            existing.insert(item.id.clone());
                            app.connections.push(item);
                            added += 1;
                        }
                    }
                    if added > 0 {
                        app.storage.save_connections(&app.connections);
                    }
                    window.push_notification(
                        Notification::success(i18n::tf(
                            "导入 {n} 条，跳过 {n2} 条",
                            &[
                                ("n", &added.to_string()),
                                ("n2", &skipped.to_string()),
                            ],
                        )),
                        cx,
                    );
                }
            },
            window,
            cx,
        );
    }

    // ── 对话框 ────────────────────────────────────────────────────────────
}
