use crate::{
    intelligence::{self, Intelligence, JobStatus, ModelStatus},
    library::{Asset, Group, ImportReport, Library, Settings, Snapshot},
    media, Result,
};
use clipboard_rs::{
    common::RustImage, Clipboard, ClipboardContent, ClipboardContext, RustImageData,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

pub struct AppState {
    library: Mutex<Library>,
    models: PathBuf,
    intelligence: Mutex<Option<Intelligence>>,
    clipboard: Mutex<Option<ClipboardContext>>,
    job: Mutex<JobStatus>,
    import_job: Mutex<JobStatus>,
    cancel: AtomicBool,
}
type CommandResult<T> = std::result::Result<T, String>;
enum ModelInstall {
    Download,
    Local(Vec<PathBuf>),
}
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}
impl AppState {
    fn with_models<T>(&self, work: impl FnOnce(&mut Intelligence) -> Result<T>) -> Result<T> {
        let mut models = self.intelligence.lock().unwrap();
        if models.is_none() {
            *models = Some(Intelligence::load(&self.models)?);
        }
        work(models.as_mut().unwrap())
    }
    fn auto_index(self: &Arc<Self>) -> Result<()> {
        let enabled = self.library.lock().unwrap().settings()?.auto_index;
        if enabled
            && !self.cancel.load(Ordering::Relaxed)
            && intelligence::missing(&self.models).is_empty()
        {
            self.start_job(None, None, false)?;
        }
        Ok(())
    }
    fn start_job(
        self: &Arc<Self>,
        install: Option<ModelInstall>,
        id: Option<String>,
        explicit: bool,
    ) -> Result<()> {
        let mut job = self.job.lock().unwrap();
        if job.running {
            return if explicit {
                Err("已有任务运行中，请等待或暂停后再试".into())
            } else {
                Ok(())
            };
        }
        if install.is_none() && !intelligence::missing(&self.models).is_empty() {
            return Err("请先在偏好设置中导入或下载本地搜索模型".into());
        }
        if !explicit && install.is_none() && self.library.lock().unwrap().pending_count(None)? == 0
        {
            return Ok(());
        }
        if let Some(id) = &id {
            let library = self.library.lock().unwrap();
            library.asset(id)?;
            library.db.execute(
                "UPDATE assets SET ocr_state='pending',index_state='pending' WHERE id=?1",
                [id],
            )?;
        } else if explicit && install.is_none() {
            self.library.lock().unwrap().db.execute(
                "UPDATE assets SET
                   ocr_state=CASE WHEN ocr_state='error' THEN 'pending' ELSE ocr_state END,
                   index_state=CASE WHEN index_state='error' THEN 'pending' ELSE index_state END",
                [],
            )?;
        }
        *job = JobStatus {
            running: true,
            message: match &install {
                Some(ModelInstall::Download) => "准备下载模型",
                Some(ModelInstall::Local(_)) => "校验本地模型文件",
                None => "准备建立索引",
            }
            .into(),
            ..Default::default()
        };
        self.cancel.store(false, Ordering::Relaxed);
        let state = Arc::clone(self);
        std::thread::spawn(move || {
            let mut result = (|| -> Result<String> {
                if let Some(install) = install {
                    let progress = |done, total, message| {
                        let mut job = state.job.lock().unwrap();
                        job.done = done;
                        job.total = total;
                        job.message = message;
                    };
                    match install {
                        ModelInstall::Download => {
                            intelligence::download(&state.models, &state.cancel, progress)?
                        }
                        ModelInstall::Local(paths) => intelligence::import_local(
                            &state.models,
                            &paths,
                            &state.cancel,
                            progress,
                        )?,
                    }
                    if state.cancel.load(Ordering::Relaxed) {
                        return Ok("任务已暂停".into());
                    }
                    let missing = intelligence::missing(&state.models);
                    if !missing.is_empty() {
                        return Ok(format!("已导入所选文件，还缺少：{}", missing.join("、")));
                    }
                    // Verify that the installed files can actually be loaded before indexing.
                    state.with_models(|_| Ok(()))?;
                }
                {
                    let mut job = state.job.lock().unwrap();
                    job.done = 0;
                    job.total = 0;
                }
                state.index_pending(id.as_deref())?;
                Ok("整理完成".into())
            })();
            loop {
                let mut job = state.job.lock().unwrap();
                // Finish under the same lock used to start workers. An import arriving
                // at the end either joins this drain or starts the next worker.
                let more = (|| -> Result<bool> {
                    if result.is_err()
                        || state.cancel.load(Ordering::Relaxed)
                        || !intelligence::missing(&state.models).is_empty()
                    {
                        return Ok(false);
                    }
                    let library = state.library.lock().unwrap();
                    Ok(library.settings()?.auto_index && library.pending_count(None)? > 0)
                })();
                match more {
                    Ok(true) => {
                        drop(job);
                        result = state.index_pending(None).map(|()| "整理完成".into());
                        continue;
                    }
                    Err(error) => result = Err(error),
                    Ok(false) => {}
                }
                job.running = false;
                match result {
                    Err(error) => {
                        job.error = Some(error.to_string());
                        job.message = "任务未完成".into();
                    }
                    Ok(message) => {
                        job.message = if state.cancel.load(Ordering::Relaxed) {
                            "任务已暂停，可继续处理".into()
                        } else if job.error.is_some() {
                            "处理完成，部分项目失败".into()
                        } else {
                            message
                        };
                    }
                }
                break;
            }
        });
        Ok(())
    }
    fn index_pending(&self, only: Option<&str>) -> Result<()> {
        if self.library.lock().unwrap().pending_count(only)? == 0 {
            return Ok(());
        }
        // Load once before draining: a broken installation must not fail every item.
        self.with_models(|_| Ok(()))?;
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let (asset, pending) = {
                let library = self.library.lock().unwrap();
                (library.next_pending(only)?, library.pending_count(only)?)
            };
            let Some(asset) = asset else {
                return Ok(());
            };
            {
                let mut job = self.job.lock().unwrap();
                job.total = job.done + pending;
            }
            if self.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            self.job.lock().unwrap().message = format!("识别与索引 · {}", asset.name);
            if asset.ocr_state == "pending" {
                match self.with_models(|models| models.recognize(Path::new(&asset.path))) {
                    Ok(text) => self.library.lock().unwrap().update_ocr(&asset.id, &text)?,
                    Err(error) => {
                        self.library.lock().unwrap().mark_error(&asset.id, true)?;
                        self.job.lock().unwrap().error = Some(format!("{}：{error}", asset.name));
                    }
                }
            }
            // Read again so edits made during OCR are included in the embedding.
            if self.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let current = self.library.lock().unwrap().find_asset(&asset.id)?;
            if let Some(current) = current.filter(|asset| asset.index_state == "pending") {
                let source = Library::search_text(&current);
                match self.with_models(|models| models.embed(&source, false)) {
                    Ok(vector) => {
                        self.library
                            .lock()
                            .unwrap()
                            .put_embedding(&asset.id, &source, &vector)?;
                    }
                    Err(error) => {
                        self.library.lock().unwrap().mark_error(&asset.id, false)?;
                        self.job.lock().unwrap().error = Some(format!("{}：{error}", asset.name));
                    }
                }
            }
            self.job.lock().unwrap().done += 1;
        }
    }
    fn import_paths(
        self: &Arc<Self>,
        paths: &[String],
        group: Option<i64>,
    ) -> Result<ImportReport> {
        {
            let mut job = self.import_job.lock().unwrap();
            if job.running {
                return Err("已有图片正在导入".into());
            }
            *job = JobStatus {
                running: true,
                total: paths.len(),
                ..Default::default()
            };
        }
        let result = (|| -> Result<ImportReport> {
            let mut report = ImportReport::default();
            for (index, path) in paths.iter().enumerate() {
                // Release the library between files so browsing and indexing can proceed.
                let item = self
                    .library
                    .lock()
                    .unwrap()
                    .import_files(std::slice::from_ref(path), group);
                report.added += item.added;
                report.duplicates += item.duplicates;
                report.errors.extend(item.errors);
                {
                    let mut job = self.import_job.lock().unwrap();
                    job.done = index + 1;
                    job.message = format!("已导入 {} 张", report.added);
                }
                if item.added > 0 {
                    self.auto_index()?;
                }
            }
            Ok(report)
        })();
        let mut job = self.import_job.lock().unwrap();
        job.running = false;
        match &result {
            Ok(report) => job.error = report.errors.last().cloned(),
            Err(error) => job.error = Some(error.to_string()),
        }
        result
    }
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
    blocking(move || {
        if query.chars().count() > 4000 {
            return Err("搜索内容过长".into());
        }
        let mut embedding = None;
        if !query.trim().is_empty() && mode != "keyword" {
            if intelligence::missing(&state.models).is_empty() {
                embedding = Some(state.with_models(|models| models.embed(&query, true))?);
            } else if mode == "semantic" {
                return Err("语义搜索需要本地模型，请先导入或下载模型并建立索引".into());
            }
        }
        state
            .library
            .lock()
            .unwrap()
            .search(&query, &mode, group_id, embedding.as_deref())
    })
    .await
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
    id: String,
    size: Option<u32>,
) -> CommandResult<String> {
    let state = Arc::clone(state.inner());
    blocking(move || {
        // Serializing copies prevents two commands from writing the same cache file.
        let mut clipboard = state.clipboard.lock().unwrap();
        let (asset, settings, root) = {
            let library = state.library.lock().unwrap();
            (
                library.asset(&id)?,
                library.settings()?,
                library.root.clone(),
            )
        };
        let size = size.unwrap_or(settings.copy_size);
        let path = media::prepare_copy(&asset, &root, size, settings.allow_upscale)?;
        if clipboard.is_none() {
            *clipboard = Some(ClipboardContext::new()?);
        }
        let mut contents = vec![ClipboardContent::Files(vec![path
            .to_string_lossy()
            .into_owned()])];
        if asset.animated {
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("gif");
            let format = match extension {
                "gif" => {
                    if cfg!(target_os = "macos") {
                        "com.compuserve.gif"
                    } else if cfg!(target_os = "windows") {
                        "GIF"
                    } else {
                        "image/gif"
                    }
                }
                "webp" => "image/webp",
                _ => {
                    if cfg!(target_os = "macos") {
                        "public.png"
                    } else {
                        "image/png"
                    }
                }
            };
            contents.push(ClipboardContent::Other(
                format.into(),
                std::fs::read(&path)?,
            ));
        } else {
            let preview = media::inspect(&std::fs::read(&path)?)?.preview;
            contents.push(ClipboardContent::Image(RustImageData::from_dynamic_image(
                preview,
            )));
        }
        clipboard.as_ref().unwrap().set(contents)?;
        Ok(if asset.animated {
            "已复制动图文件；粘贴效果取决于目标应用的文件粘贴支持".into()
        } else {
            "已复制图片".into()
        })
    })
    .await
}
#[tauri::command]
fn model_status(state: State<'_, Arc<AppState>>) -> ModelStatus {
    let missing = intelligence::missing(&state.models);
    ModelStatus {
        ready: missing.is_empty(),
        directory: state.models.to_string_lossy().into_owned(),
        missing,
        job: state.job.lock().unwrap().clone(),
        import: state.import_job.lock().unwrap().clone(),
    }
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
    state.cancel.store(true, Ordering::Relaxed);
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            let library =
                Library::open(&root).map_err(|error| error as Box<dyn std::error::Error>)?;
            let state = Arc::new(AppState {
                library: Mutex::new(library),
                models: root.join("models"),
                intelligence: Mutex::new(None),
                clipboard: Mutex::new(None),
                job: Mutex::new(JobStatus::default()),
                import_job: Mutex::new(JobStatus::default()),
                cancel: AtomicBool::new(false),
            });
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
            pause_index
        ])
        .run(tauri::generate_context!())
        .expect("无法启动 Memechan");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_until(ready: impl Fn() -> bool) -> Result<()> {
        let start = Instant::now();
        while !ready() {
            if start.elapsed() > Duration::from_secs(60) {
                return Err("后台索引测试超时".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    #[test]
    #[ignore = "offline model inference; set MEMECHAN_MODEL_TEST_DIR"]
    fn bulk_import_joins_active_worker_and_resumes_after_restart() -> Result<()> {
        let models = PathBuf::from(std::env::var("MEMECHAN_MODEL_TEST_DIR")?);
        let directory = tempfile::tempdir()?;
        let root = directory.path().join("library");
        let state = Arc::new(AppState {
            library: Mutex::new(Library::open(&root)?),
            intelligence: Mutex::new(Some(Intelligence::load(&models)?)),
            models: models.clone(),
            clipboard: Mutex::new(None),
            job: Mutex::new(JobStatus::default()),
            import_job: Mutex::new(JobStatus::default()),
            cancel: AtomicBool::new(true),
        });
        let mut paths = Vec::new();
        for index in 0..32 {
            let path = directory.path().join(format!("meme-{index}.png"));
            image::RgbaImage::from_pixel(4, 4, image::Rgba([index, 100, 150, 255])).save(&path)?;
            paths.push(path.to_string_lossy().into_owned());
        }
        assert_eq!(state.import_paths(&paths[..1], None)?.added, 1);
        let first = state.library.lock().unwrap().next_pending(None)?.unwrap();
        // Hold inference while a single-item job starts, then append a bulk import.
        let inference = state.intelligence.lock().unwrap();
        state.start_job(None, Some(first.id.clone()), true)?;
        assert_eq!(state.import_paths(&paths[1..], None)?.added, 31);
        assert_eq!(state.library.lock().unwrap().snapshot()?.assets.len(), 32);
        drop(inference);

        wait_until(|| state.job.lock().unwrap().done > 0)?;
        state.cancel.store(true, Ordering::Relaxed);
        wait_until(|| !state.job.lock().unwrap().running)?;
        let ready = state.library.lock().unwrap().asset(&first.id)?;
        assert_eq!(ready.index_state, "ready");
        let remaining = state.library.lock().unwrap().pending_count(None)?;
        assert!(remaining > 0 && remaining < 32);
        // New imports cannot silently unpause an explicitly paused queue.
        let extra = directory.path().join("extra.png");
        image::RgbaImage::new(4, 4).save(&extra)?;
        assert_eq!(
            state
                .import_paths(&[extra.to_string_lossy().into_owned()], None)?
                .added,
            1
        );
        assert!(!state.job.lock().unwrap().running);
        let broken = state
            .library
            .lock()
            .unwrap()
            .assets()?
            .into_iter()
            .find(|asset| asset.ocr_state == "pending")
            .unwrap();
        std::fs::write(&broken.path, b"invalid image")?;

        let intelligence = state.intelligence.lock().unwrap().take();
        drop(state);
        let restarted = Arc::new(AppState {
            library: Mutex::new(Library::open(&root)?),
            models,
            intelligence: Mutex::new(intelligence),
            clipboard: Mutex::new(None),
            job: Mutex::new(JobStatus::default()),
            import_job: Mutex::new(JobStatus::default()),
            cancel: AtomicBool::new(false),
        });
        restarted.auto_index()?;
        wait_until(|| !restarted.job.lock().unwrap().running)?;
        let library = restarted.library.lock().unwrap();
        assert_eq!(library.pending_count(None)?, 0);
        assert_eq!(library.asset(&broken.id)?.ocr_state, "error");
        assert_eq!(
            library
                .assets()?
                .iter()
                .filter(|a| a.index_state == "ready")
                .count(),
            33
        );
        assert!(restarted.job.lock().unwrap().error.is_some());
        assert_eq!(restarted.job.lock().unwrap().done, remaining + 1);
        drop(library);

        // A bulk arrival during a targeted reindex must be picked up without a restart.
        let inference = restarted.intelligence.lock().unwrap();
        restarted.start_job(None, Some(first.id), true)?;
        let arrived = directory.path().join("arrived.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 23, 255])).save(&arrived)?;
        assert_eq!(
            restarted
                .import_paths(&[arrived.to_string_lossy().into_owned()], None)?
                .added,
            1
        );
        drop(inference);
        wait_until(|| !restarted.job.lock().unwrap().running)?;
        assert_eq!(restarted.library.lock().unwrap().pending_count(None)?, 0);
        assert_eq!(restarted.job.lock().unwrap().done, 2);
        Ok(())
    }
}
