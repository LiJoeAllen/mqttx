fn main() {
    // Workaround for tauri-build issue with cargo's OUT_DIR format change.
    // The newer cargo version uses `{package}/{hash}/out` instead of `{package}-{hash}/out`,
    // which breaks the target_dir calculation in tauri-build 2.6.3.
    // We create the expected `build/build` directory to prevent the fs::read_dir error.
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = std::path::Path::new(&out_dir);
    if let Some(parent) = out_path.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) {
        let build_build = parent.join("build");
        let _ = std::fs::create_dir_all(&build_build);
    }
    
    tauri_build::build()
}