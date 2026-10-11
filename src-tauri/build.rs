use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=ORT_LIB_PATH");
    let library_dir = PathBuf::from(env::var_os("ORT_LIB_PATH").expect(
        "未设置 ORT_LIB_PATH。请从项目目录运行 Cargo，或显式指定 ONNX Runtime 静态库目录。",
    ));
    let windows = env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows";
    let library = library_dir.join(if windows {
        "onnxruntime.lib"
    } else {
        "libonnxruntime.a"
    });
    assert!(
        library.is_file(),
        "缺少 ONNX Runtime 静态库：{}。请先运行 pnpm setup:ort --download，或 pnpm setup:ort --archive <本地压缩包>；编译不会自动下载。",
        library.display()
    );
    println!("cargo:rerun-if-changed={}", library.display());

    // ort-sys treats ORT_LIB_PATH as user-provided: its Pyke-specific DirectML
    // linking and DLL copying no longer run, so preserve them for Windows builds.
    if windows && library_dir.join("DirectML.lib").is_file() {
        println!("cargo:rustc-link-search=native={}", library_dir.display());
        for lib in ["dxguid", "DXCORE", "DXGI", "D3D12", "DirectML"] {
            println!("cargo:rustc-link-lib={lib}");
        }
        let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
        let target_dir = out_dir.ancestors().nth(3).unwrap();
        for entry in fs::read_dir(&library_dir).expect("无法读取 ONNX Runtime 目录") {
            let entry = entry.expect("无法读取 ONNX Runtime 文件");
            if entry.path().extension().is_some_and(|ext| ext == "dll") {
                println!("cargo:rerun-if-changed={}", entry.path().display());
                for dir in [target_dir.to_path_buf(), target_dir.join("deps")] {
                    fs::create_dir_all(&dir).expect("无法创建 DLL 输出目录");
                    let destination = dir.join(entry.file_name());
                    if destination.is_symlink() {
                        fs::remove_file(&destination).expect("无法移除旧 DLL 链接");
                    }
                    fs::copy(entry.path(), destination).expect("复制 DLL 失败");
                }
            }
        }
    }

    #[cfg(feature = "desktop")]
    tauri_build::build();
}
