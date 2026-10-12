use crate::{
    app::{AppState, ModelStatus, TaskAction, VisionStatus},
    clipboard::ClipboardState,
    copy_cache::{CacheUsage, CopyCache, LIMIT_BYTES},
    library::{Asset, Group, ImportReport, Settings, Snapshot, TaskKind},
    vision::{Description, VisionConfig},
    Result,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{Manager, State, WindowEvent};
use tauri_plugin_dialog::DialogExt;
mod tray;

type CommandResult<T> = std::result::Result<T, String>;
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}
#[tauri::command]
async fn snapshot(state: State<'_, Arc<AppState>>) -> CommandResult<Snapshot> {
    let state = Arc::clone(state.inner());
    blocking(move || state.library_read.lock().unwrap().snapshot()).await
}
#[tauri::command]
fn invalidate_searches(state: State<'_, Arc<AppState>>) -> u64 {
    state.invalidate_searches()
}
#[tauri::command]
async fn search(
    state: State<'_, Arc<AppState>>,
    generation: u64,
    query: String,
    mode: String,
    group_id: Option<i64>,
) -> CommandResult<Option<Vec<Asset>>> {
    let state = Arc::clone(state.inner());
    blocking(move || state.search(generation, &query, &mode, group_id)).await
}
#[tauri::command]
async fn import_files(
    state: State<'_, Arc<AppState>>,
    paths: Vec<String>,
    group_id: Option<i64>,
) -> CommandResult<ImportReport> {
    let state = Arc::clone(state.inner());
    blocking(move || state.import_paths(&paths, group_id)).await
}
#[tauri::command]
async fn pick_import(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    group_id: Option<i64>,
) -> CommandResult<ImportReport> {
    let state = Arc::clone(state.inner());
    blocking(move || {
        let Some(files) = app
            .dialog()
            .file()
            .add_filter("表情图片", &["png", "jpg", "jpeg", "gif", "webp"])
            .blocking_pick_files()
        else {
            return Ok(ImportReport::default());
        };
        let paths = files
            .into_iter()
            .map(|file| file.into_path().map(|p| p.to_string_lossy().into_owned()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        state.import_paths(&paths, group_id)
    })
    .await
}
#[tauri::command]
async fn save_asset(
    state: State<'_, Arc<AppState>>,
    id: String,
    description: String,
    favorite: bool,
    groups: Vec<i64>,
) -> CommandResult<()> {
    let state = Arc::clone(state.inner());
    blocking(move || {
        state
            .library_write
            .lock()
            .unwrap()
            .save_asset(&id, &description, favorite, &groups)?;
        state.auto_index()
    })
    .await
}
#[tauri::command]
async fn delete_asset(state: State<'_, Arc<AppState>>, id: String) -> CommandResult<()> {
    let state = Arc::clone(state.inner());
    blocking(move || state.library_write.lock().unwrap().delete_asset(&id)).await
}
#[tauri::command]
async fn delete_assets(state: State<'_, Arc<AppState>>, ids: Vec<String>) -> CommandResult<()> {
    let state = Arc::clone(state.inner());
    blocking(move || state.library_write.lock().unwrap().delete_assets(&ids)).await
}
#[tauri::command]
async fn set_assets_group(
    state: State<'_, Arc<AppState>>,
    ids: Vec<String>,
    group_id: i64,
    add: bool,
) -> CommandResult<()> {
    let state = Arc::clone(state.inner());
    blocking(move || {
        state
            .library_write
            .lock()
            .unwrap()
            .set_assets_group(&ids, group_id, add)
    })
    .await
}
#[tauri::command]
async fn save_group(
    state: State<'_, Arc<AppState>>,
    id: Option<i64>,
    name: String,
    color: String,
) -> CommandResult<Group> {
    let state = Arc::clone(state.inner());
    blocking(move || {
        state
            .library_write
            .lock()
            .unwrap()
            .save_group(id, &name, &color)
    })
    .await
}
#[tauri::command]
async fn delete_group(state: State<'_, Arc<AppState>>, id: i64) -> CommandResult<()> {
    let state = Arc::clone(state.inner());
    blocking(move || state.library_write.lock().unwrap().delete_group(id)).await
}
#[tauri::command]
async fn save_settings(state: State<'_, Arc<AppState>>, settings: Settings) -> CommandResult<()> {
    let state = Arc::clone(state.inner());
    blocking(move || state.save_settings(settings)).await
}
#[tauri::command]
async fn copy_asset(
    state: State<'_, Arc<AppState>>,
    clipboard: State<'_, Arc<ClipboardState>>,
    cache: State<'_, Arc<Mutex<CopyCache>>>,
    id: String,
    size: Option<u32>,
) -> CommandResult<String> {
    let state = Arc::clone(state.inner());
    let clipboard = Arc::clone(clipboard.inner());
    let cache = Arc::clone(cache.inner());
    blocking(move || {
        let (asset, settings, root) = {
            let library = state.library_read.lock().unwrap();
            (
                library.asset(&id)?,
                library.settings()?,
                library.root.clone(),
            )
        };
        // Lock cache before clipboard, through file preparation and clipboard publication.
        let _cache = cache.lock().unwrap();
        clipboard.copy(&asset, &root, &settings, size)
    })
    .await
}

#[tauri::command]
async fn copy_cache_status(cache: State<'_, Arc<Mutex<CopyCache>>>) -> CommandResult<CacheUsage> {
    let cache = Arc::clone(cache.inner());
    blocking(move || cache.lock().unwrap().usage()).await
}

#[tauri::command]
async fn clear_copy_cache(
    cache: State<'_, Arc<Mutex<CopyCache>>>,
    clipboard: State<'_, Arc<ClipboardState>>,
) -> CommandResult<CacheUsage> {
    let cache = Arc::clone(cache.inner());
    let clipboard = Arc::clone(clipboard.inner());
    blocking(move || {
        cache
            .lock()
            .unwrap()
            .clean(0, || clipboard.referenced_files())
    })
    .await
}

#[tauri::command]
async fn model_status(state: State<'_, Arc<AppState>>) -> CommandResult<ModelStatus> {
    let state = Arc::clone(state.inner());
    blocking(move || state.model_status()).await
}
#[tauri::command]
async fn vision_status(state: State<'_, Arc<AppState>>) -> CommandResult<VisionStatus> {
    let state = Arc::clone(state.inner());
    blocking(move || state.vision_status()).await
}
#[tauri::command]
async fn save_vision_config(
    state: State<'_, Arc<AppState>>,
    config: VisionConfig,
) -> CommandResult<()> {
    let state = Arc::clone(state.inner());
    blocking(move || state.save_vision_config(config)).await
}
#[tauri::command]
async fn test_vision(state: State<'_, Arc<AppState>>) -> CommandResult<Description> {
    let state = Arc::clone(state.inner());
    blocking(move || state.test_vision()).await
}
#[tauri::command]
async fn control_task(
    state: State<'_, Arc<AppState>>,
    kind: TaskKind,
    action: TaskAction,
    id: Option<String>,
) -> CommandResult<()> {
    let state = Arc::clone(state.inner());
    blocking(move || state.control_task(kind, action, id.as_deref())).await
}
#[tauri::command]
async fn clear_text(
    state: State<'_, Arc<AppState>>,
    kind: TaskKind,
    id: Option<String>,
) -> CommandResult<usize> {
    let state = Arc::clone(state.inner());
    blocking(move || state.clear_text(kind, id.as_deref())).await
}
#[tauri::command]
async fn import_models(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> CommandResult<bool> {
    let state = Arc::clone(state.inner());
    blocking(move || {
        let Some(files) = app
            .dialog()
            .file()
            .set_title("选择 Qwen 模型清单或 OCR 文件")
            .add_filter("Qwen 模型清单 / OCR 模型", &["json", "onnx"])
            .blocking_pick_files()
        else {
            return Ok(false);
        };
        let paths = files
            .into_iter()
            .map(|file| file.into_path())
            .collect::<std::result::Result<Vec<_>, _>>()?;
        state.start_install(paths)?;
        Ok(true)
    })
    .await
}
#[tauri::command]
fn pause_install(state: State<'_, Arc<AppState>>) {
    state.pause_install();
}

pub fn run() {
    let mut builder = tauri::Builder::default();
    // Register first, before database/background task setup. `dev --release` is still dev.
    if !tauri::is_dev() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
            tray::show_main_window(app);
        }));
    }
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            let state = Arc::new(
                AppState::new(&root).map_err(|error| error as Box<dyn std::error::Error>)?,
            );
            let clipboard = Arc::new(ClipboardState::default());
            let cache = Arc::new(Mutex::new(CopyCache::new(root)));
            let cache_worker = Arc::downgrade(&cache);
            let clipboard_worker = Arc::downgrade(&clipboard);
            std::thread::spawn(move || loop {
                let (Some(cache), Some(clipboard)) =
                    (cache_worker.upgrade(), clipboard_worker.upgrade())
                else {
                    break;
                };
                // Keep the cache locked from reading protected paths through deleting files.
                if let Err(error) = cache
                    .lock()
                    .unwrap()
                    .clean(LIMIT_BYTES, || clipboard.referenced_files())
                {
                    eprintln!("复制缓存回收失败：{error}");
                }
                drop(cache);
                drop(clipboard);
                std::thread::sleep(Duration::from_secs(60));
            });
            app.manage(cache);
            app.manage(clipboard);
            state
                .auto_index()
                .map_err(|error| error as Box<dyn std::error::Error>)?;
            app.manage(state);
            tray::setup(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    match window.hide() {
                        Ok(()) => api.prevent_close(),
                        Err(error) => eprintln!("收起到托盘失败：{error}"),
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            search,
            invalidate_searches,
            import_files,
            pick_import,
            save_asset,
            delete_asset,
            delete_assets,
            set_assets_group,
            save_group,
            delete_group,
            save_settings,
            copy_asset,
            copy_cache_status,
            clear_copy_cache,
            model_status,
            import_models,
            control_task,
            clear_text,
            pause_install,
            vision_status,
            save_vision_config,
            test_vision
        ])
        .build(tauri::generate_context!())
        .expect("无法启动 Memechan");
    // Plugins (including single-instance) are ready, but windows and WebViews
    // are only created by run(). Migrate before anything opens the data directory.
    let legacy = app
        .path()
        .data_dir()
        .expect("无法确定旧数据目录")
        .join("com.emojidesktop.app");
    let root = app.path().app_data_dir().expect("无法确定应用数据目录");
    crate::migration::migrate(&legacy, &root).expect("无法迁移旧版资料库");
    app.run(|_app, _event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = _event {
            tray::show_main_window(_app);
        }
    });
}
