use crate::{
    app::{AppState, ModelInstall, ModelStatus, VisionStatus},
    clipboard::ClipboardState,
    library::{Asset, Group, ImportReport, Settings, Snapshot},
    vision::{Description, VisionConfig},
    Result,
};
use std::sync::Arc;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
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
    blocking(move || state.library.lock().unwrap().snapshot()).await
}
#[tauri::command]
async fn search(
    state: State<'_, Arc<AppState>>,
    query: String,
    mode: String,
    group_id: Option<i64>,
) -> CommandResult<Vec<Asset>> {
    let state = Arc::clone(state.inner());
    blocking(move || state.search(&query, &mode, group_id)).await
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
            .library
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
    blocking(move || state.library.lock().unwrap().delete_asset(&id)).await
}
#[tauri::command]
async fn save_group(
    state: State<'_, Arc<AppState>>,
    id: Option<i64>,
    name: String,
    color: String,
) -> CommandResult<Group> {
    let state = Arc::clone(state.inner());
    blocking(move || state.library.lock().unwrap().save_group(id, &name, &color)).await
}
#[tauri::command]
async fn delete_group(state: State<'_, Arc<AppState>>, id: i64) -> CommandResult<()> {
    let state = Arc::clone(state.inner());
    blocking(move || state.library.lock().unwrap().delete_group(id)).await
}
#[tauri::command]
async fn save_settings(state: State<'_, Arc<AppState>>, settings: Settings) -> CommandResult<()> {
    let state = Arc::clone(state.inner());
    blocking(move || {
        state.library.lock().unwrap().save_settings(settings)?;
        state.auto_index()
    })
    .await
}
#[tauri::command]
async fn copy_asset(
    state: State<'_, Arc<AppState>>,
    clipboard: State<'_, Arc<ClipboardState>>,
    id: String,
    size: Option<u32>,
) -> CommandResult<String> {
    let state = Arc::clone(state.inner());
    let clipboard = Arc::clone(clipboard.inner());
    blocking(move || {
        let (asset, settings, root) = {
            let library = state.library.lock().unwrap();
            (
                library.asset(&id)?,
                library.settings()?,
                library.root.clone(),
            )
        };
        clipboard.copy(&asset, &root, &settings, size)
    })
    .await
}

#[tauri::command]
fn model_status(state: State<'_, Arc<AppState>>) -> ModelStatus {
    state.model_status()
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
    api_key: Option<String>,
) -> CommandResult<()> {
    let state = Arc::clone(state.inner());
    blocking(move || state.save_vision_config(config, api_key)).await
}
#[tauri::command]
async fn test_vision(state: State<'_, Arc<AppState>>) -> CommandResult<Description> {
    let state = Arc::clone(state.inner());
    blocking(move || state.test_vision()).await
}
#[tauri::command]
async fn start_vision(state: State<'_, Arc<AppState>>, id: Option<String>) -> CommandResult<()> {
    let state = Arc::clone(state.inner());
    blocking(move || state.start_vision(id.as_deref(), true)).await
}
#[tauri::command]
fn pause_vision(state: State<'_, Arc<AppState>>) {
    state.pause_vision();
}
#[tauri::command]
fn install_models(state: State<'_, Arc<AppState>>) -> CommandResult<()> {
    state
        .start_job(Some(ModelInstall::Download), None, true)
        .map_err(|e| e.to_string())
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
            .set_title("导入本地搜索模型（可多选）")
            .add_filter("模型与分词器", &["onnx", "json"])
            .blocking_pick_files()
        else {
            return Ok(false);
        };
        let paths = files
            .into_iter()
            .map(|file| file.into_path())
            .collect::<std::result::Result<Vec<_>, _>>()?;
        state.start_job(Some(ModelInstall::Local(paths)), None, true)?;
        Ok(true)
    })
    .await
}
#[tauri::command]
fn start_index(state: State<'_, Arc<AppState>>, id: Option<String>) -> CommandResult<()> {
    state.start_job(None, id, true).map_err(|e| e.to_string())
}
#[tauri::command]
fn pause_index(state: State<'_, Arc<AppState>>) {
    state.pause_index();
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            let state = Arc::new(
                AppState::new(&root).map_err(|error| error as Box<dyn std::error::Error>)?,
            );
            app.manage(Arc::new(ClipboardState::default()));
            state
                .auto_index()
                .map_err(|error| error as Box<dyn std::error::Error>)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            search,
            import_files,
            pick_import,
            save_asset,
            delete_asset,
            save_group,
            delete_group,
            save_settings,
            copy_asset,
            model_status,
            install_models,
            import_models,
            start_index,
            pause_index,
            vision_status,
            save_vision_config,
            test_vision,
            start_vision,
            pause_vision
        ])
        .run(tauri::generate_context!())
        .expect("无法启动 Memechan");
}
