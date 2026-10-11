use crate::{
    import::{read_source, PreparedImport},
    inference_scheduler::InferenceScheduler,
    intelligence::{self, Intelligence},
    library::{Asset, ImportReport, Library, Settings, TaskKind},
    vision::{Description, VisionClient, VisionConfig},
    Result,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

pub struct AppState {
    // Independent connections to the same WAL database. Never hold both DB locks together.
    // Read lock: short queries and row materialization only. Release before file/network I/O,
    // model loading/inference, or search ranking. Keep read transactions short.
    pub(crate) library_read: Mutex<Library>,
    // Read-modify-write operations use this connection for both validation and mutation.
    pub(crate) library_write: Mutex<Library>,
    models: PathBuf,
    intelligence: Mutex<Option<Intelligence>>,
    inference_scheduler: InferenceScheduler,
    local: Mutex<LocalTasks>,
    job: Mutex<JobStatus>,
    import_job: Mutex<JobStatus>,
    cancel: AtomicBool,
    vision: Mutex<VisionSession>,
}
#[derive(Default)]
struct LocalTasks {
    worker_running: bool,
    ocr: JobStatus,
    index: JobStatus,
    index_only: Option<String>,
}
impl LocalTasks {
    fn job(&mut self, kind: TaskKind) -> &mut JobStatus {
        match kind {
            TaskKind::Ocr => &mut self.ocr,
            TaskKind::Index => &mut self.index,
            TaskKind::Vision => unreachable!("vision uses its own HTTP queue"),
        }
    }
}
#[derive(Default)]
struct VisionSession {
    client: Option<VisionClient>,
    job: JobStatus,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionStatus {
    config: VisionConfig,
    active: bool,
}
#[derive(Clone, Default, Serialize)]
pub struct JobStatus {
    pub running: bool,
    pub paused: bool,
    pub done: usize,
    pub total: usize,
    pub pending: usize,
    pub failed: usize,
    pub completed: usize,
    pub incomplete: usize,
    pub message: String,
    pub error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub library_revision: u64,
    pub ready: bool,
    pub ocr_ready: bool,
    pub ocr_enabled: bool,
    pub semantic_enabled: bool,
    pub directory: String,
    pub missing: Vec<String>,
    pub job: JobStatus,
    pub import: JobStatus,
    pub ocr: JobStatus,
    pub index: JobStatus,
    pub vision: JobStatus,
}
#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskAction {
    Pause,
    Resume,
    Retry,
    Generate,
}

impl AppState {
    pub fn new(root: &Path) -> Result<Self> {
        let mut library = Library::open(root)?;
        let model_error = Self::sync_model_version(&mut library)?;
        let client = library
            .vision_config()?
            .map(VisionClient::new)
            .transpose()?;
        Ok(Self {
            library_read: Mutex::new(Library::open_read_only(root)?),
            library_write: Mutex::new(library),
            models: root.join("models"),
            intelligence: Mutex::new(None),
            inference_scheduler: InferenceScheduler::default(),
            local: Mutex::new(LocalTasks {
                index: JobStatus {
                    error: model_error,
                    ..Default::default()
                },
                ..Default::default()
            }),
            job: Mutex::new(JobStatus::default()),
            import_job: Mutex::new(JobStatus::default()),
            cancel: AtomicBool::new(false),
            vision: Mutex::new(VisionSession {
                client,
                ..Default::default()
            }),
        })
    }
    fn sync_model_version(library: &mut Library) -> Result<Option<String>> {
        let model = library.settings()?.embedding_model;
        match intelligence::model_version(&library.root.join("models"), model) {
            Ok(version) => {
                library.set_embedding_version(&version)?;
                Ok(None)
            }
            // Model errors are exposed in task/model status; storage errors still propagate.
            Err(error) => Ok(Some(format!("语义模型不可用：{error}"))),
        }
    }
    fn with_models<T>(
        &self,
        kind: TaskKind,
        work: impl FnOnce(&mut Intelligence) -> Result<T>,
    ) -> Result<Option<T>> {
        let mut models = self.intelligence.lock().unwrap();
        let (settings, database_version) = {
            let library = self.library_read.lock().unwrap();
            (library.settings()?, library.embedding_version()?)
        };
        // Check under the model lock so queued work cannot reload a disabled model.
        if kind == TaskKind::Index && !settings.semantic_enabled {
            return Ok(None);
        }
        if models.is_none() {
            *models = Some(Intelligence::load(&self.models, settings.embedding_model)?);
        }
        if kind == TaskKind::Index {
            let version = models
                .as_ref()
                .unwrap()
                .version
                .as_ref()
                .map_err(|error| error.clone())?;
            if version != &database_version {
                return Err("模型版本正在切换，请等待模型导入完成".into());
            }
        }
        work(models.as_mut().unwrap()).map(Some)
    }
    pub fn save_settings(self: &Arc<Self>, settings: Settings) -> Result<()> {
        {
            let install = self.job.lock().unwrap();
            let mut local = self.local.lock().unwrap();
            let previous = self.library_write.lock().unwrap().settings()?;
            let changed_model = previous.embedding_model != settings.embedding_model;
            let disabling_semantic = previous.semantic_enabled && !settings.semantic_enabled;
            if changed_model {
                if install.running || local.worker_running {
                    return Err("切换模型前请暂停本地任务，并等待当前图片和模型导入完成".into());
                }
                // Serialize the switch with search, so query vectors cannot cross models.
                let mut models = self.intelligence.lock().unwrap();
                let mut library = self.library_write.lock().unwrap();
                library.save_settings(settings)?;
                let error = Self::sync_model_version(&mut library)?;
                *models = None;
                local.index = JobStatus {
                    paused: local.index.paused,
                    error,
                    ..Default::default()
                };
                local.index_only = None;
            } else {
                self.library_write.lock().unwrap().save_settings(settings)?;
            }
            if disabling_semantic {
                self.inference_scheduler.invalidate_searches();
                local.index.running = false;
                local.index.message.clear();
                local.index_only = None;
                // Settings are already persisted; wait for the current inference to finish,
                // then release only the embedding session and keep OCR available.
                if let Some(models) = self.intelligence.lock().unwrap().as_mut() {
                    models.unload_embedding();
                }
            }
        }
        self.auto_index()
    }
    pub fn clear_text(self: &Arc<Self>, kind: TaskKind, id: Option<&str>) -> Result<usize> {
        let changed = self.library_write.lock().unwrap().clear_text(kind, id)?;
        self.auto_index()?;
        Ok(changed)
    }
    pub fn control_task(
        self: &Arc<Self>,
        kind: TaskKind,
        action: TaskAction,
        id: Option<&str>,
    ) -> Result<()> {
        if kind == TaskKind::Vision {
            return self.start_vision(id, Some(action));
        }
        {
            // Installation and local inference cannot start across one another.
            let install = self.job.lock().unwrap();
            if install.running && action != TaskAction::Pause {
                return Err("请等待模型安装完成".into());
            }
            let mut local = self.local.lock().unwrap();
            let job = local.job(kind);
            if action == TaskAction::Pause {
                job.paused = true;
                job.message = "当前图片处理后暂停".into();
                return Ok(());
            }
            if job.running && matches!(action, TaskAction::Retry | TaskAction::Generate) {
                return Err("请先暂停当前任务，等待当前图片处理完成".into());
            }
            let library = self.library_write.lock().unwrap();
            let settings = library.settings()?;
            if kind == TaskKind::Ocr && !settings.ocr_enabled {
                return Err("请先在偏好设置中启用本地 OCR".into());
            }
            if kind == TaskKind::Index && !settings.semantic_enabled {
                return Err("请先在偏好设置中启用语义搜索".into());
            }
            let missing = if kind == TaskKind::Ocr {
                intelligence::missing_ocr(&self.models)
            } else {
                intelligence::missing_embedding(&self.models, settings.embedding_model)
            };
            if !missing.is_empty() {
                return Err(format!("请先导入模型：{}", missing.join("、")).into());
            }
            if matches!(action, TaskAction::Retry | TaskAction::Generate) {
                library.queue_task(kind, id, action == TaskAction::Retry)?;
            }
            if job.running {
                job.paused = false;
            } else {
                let (pending, ..) = library.task_counts(kind)?;
                *job = JobStatus {
                    running: pending > 0,
                    total: pending,
                    ..Default::default()
                };
            }
            if kind == TaskKind::Index {
                local.index_only = id.map(str::to_owned);
            }
        }
        self.start_local_worker();
        Ok(())
    }
    pub fn auto_index(self: &Arc<Self>) -> Result<()> {
        {
            let install = self.job.lock().unwrap();
            if install.running {
                return Ok(());
            }
            let mut local = self.local.lock().unwrap();
            let settings = self.library_read.lock().unwrap().settings()?;
            for (kind, enabled, ready) in [
                (
                    TaskKind::Ocr,
                    settings.ocr_enabled,
                    intelligence::missing_ocr(&self.models).is_empty(),
                ),
                (
                    TaskKind::Index,
                    settings.semantic_enabled && settings.auto_index,
                    intelligence::missing_embedding(&self.models, settings.embedding_model)
                        .is_empty(),
                ),
            ] {
                let job = local.job(kind);
                if enabled && ready && !job.paused && !job.running {
                    let (pending, ..) = self.library_read.lock().unwrap().task_counts(kind)?;
                    if pending > 0 {
                        *job = JobStatus {
                            running: true,
                            total: pending,
                            ..Default::default()
                        };
                    }
                }
            }
        }
        self.start_local_worker();
        Ok(())
    }
    fn start_local_worker(self: &Arc<Self>) {
        let mut local = self.local.lock().unwrap();
        if local.worker_running || (!local.ocr.running && !local.index.running) {
            return;
        }
        local.worker_running = true;
        let state = Arc::clone(self);
        std::thread::spawn(move || {
            if let Err(error) = state.process_local() {
                let mut local = state.local.lock().unwrap();
                for kind in [TaskKind::Ocr, TaskKind::Index] {
                    let job = local.job(kind);
                    if job.running {
                        job.error = Some(error.to_string());
                        job.paused = true;
                        job.running = false;
                    }
                }
                local.worker_running = false;
            }
        });
    }
    fn process_local(self: &Arc<Self>) -> Result<()> {
        loop {
            // A queued interactive search gets the next turn before another image.
            let _permit = self.inference_scheduler.acquire_background_permit();
            let (kind, asset) = {
                let mut local = self.local.lock().unwrap();
                let library = self.library_read.lock().unwrap();
                let settings = library.settings()?;
                if local.ocr.paused || !settings.ocr_enabled {
                    local.ocr.running = false;
                }
                if settings.auto_index {
                    local.index_only = None;
                }
                if local.index.paused || !settings.semantic_enabled {
                    local.index.running = false;
                }
                // Index each completed OCR result before starting the next image.
                let mut work = None;
                if local.index.running {
                    work = library
                        .next_task(
                            TaskKind::Index,
                            local.ocr.running,
                            local.index_only.as_deref(),
                        )?
                        .map(|a| (TaskKind::Index, a));
                    if work.is_none()
                        && library
                            .next_task(TaskKind::Index, false, local.index_only.as_deref())?
                            .is_none()
                    {
                        local.index.running = false;
                    }
                }
                if work.is_none() && local.ocr.running {
                    work = library
                        .next_task(TaskKind::Ocr, false, None)?
                        .map(|a| (TaskKind::Ocr, a));
                    if work.is_none() {
                        local.ocr.running = false;
                    }
                }
                let Some((kind, asset)) = work else {
                    local.worker_running = false;
                    return Ok(());
                };
                local.job(kind).message = asset.name.clone();
                (kind, asset)
            };
            let Some(loaded) = self
                .with_models(kind, |models| match kind {
                    TaskKind::Ocr => models.ensure_ocr(),
                    TaskKind::Index => models.ensure_embedding(),
                    TaskKind::Vision => unreachable!(),
                })
                .transpose()
            else {
                continue;
            };
            if let Err(error) = loaded {
                let mut local = self.local.lock().unwrap();
                let job = local.job(kind);
                job.error = Some(error.to_string());
                job.paused = true;
                job.running = false;
                continue;
            }
            let error = if kind == TaskKind::Ocr {
                let Some(result) = self
                    .with_models(kind, |models| models.recognize(Path::new(&asset.path)))
                    .transpose()
                else {
                    continue;
                };
                let library = self.library_write.lock().unwrap();
                // Clearing text or disabling OCR during inference discards its result.
                if library.settings()?.ocr_enabled
                    && library
                        .find_asset(&asset.id)?
                        .is_some_and(|a| a.ocr_state == "pending")
                {
                    match result {
                        Ok(text) => {
                            library.update_ocr(&asset.id, &text)?;
                            None
                        }
                        Err(error) => {
                            library.mark_error(&asset.id, true)?;
                            Some(error.to_string())
                        }
                    }
                } else {
                    None
                }
            } else {
                let source = Library::search_text(&asset);
                let Some(result) = self
                    .with_models(kind, |models| models.embed(&source, false))
                    .transpose()
                else {
                    continue;
                };
                let mut library = self.library_write.lock().unwrap();
                if !library.settings()?.semantic_enabled {
                    continue;
                }
                match result {
                    Ok(vector) => {
                        library.put_embedding(&asset.id, &source, &vector)?;
                        None
                    }
                    Err(error) => {
                        if library.find_asset(&asset.id)?.is_some_and(|a| {
                            a.index_state != "idle" && Library::search_text(&a) == source
                        }) {
                            library.mark_error(&asset.id, false)?;
                        }
                        Some(error.to_string())
                    }
                }
            };
            {
                let mut local = self.local.lock().unwrap();
                let job = local.job(kind);
                if let Some(error) = error {
                    job.error = Some(format!("{}：{error}", asset.name));
                } else {
                    job.done += 1;
                }
            }
            self.auto_index()?;
        }
    }
    pub fn start_install(self: &Arc<Self>, paths: Vec<PathBuf>) -> Result<()> {
        let mut job = self.job.lock().unwrap();
        if job.running || self.local.lock().unwrap().worker_running {
            return Err("请先暂停本地任务并等待当前图片处理完成".into());
        }
        let model = self
            .library_read
            .lock()
            .unwrap()
            .settings()?
            .embedding_model;
        *job = JobStatus {
            running: true,
            message: "准备安装模型".into(),
            ..Default::default()
        };
        self.cancel.store(false, Ordering::Relaxed);
        let state = Arc::clone(self);
        std::thread::spawn(move || {
            let progress = |done, total, message| {
                let mut job = state.job.lock().unwrap();
                job.done = done;
                job.total = total;
                job.message = message;
            };
            let result =
                intelligence::import_local(&state.models, model, &paths, &state.cancel, progress)
                    .and_then(|()| {
                        let mut models = state.intelligence.lock().unwrap();
                        *models = None;
                        let mut library = state.library_write.lock().unwrap();
                        let previous_version = library.embedding_version()?;
                        let model_error = Self::sync_model_version(&mut library)?;
                        let changed_model = previous_version != library.embedding_version()?;
                        drop(library);
                        drop(models);
                        if changed_model || model_error.is_some() {
                            let mut local = state.local.lock().unwrap();
                            local.index = JobStatus {
                                paused: local.index.paused,
                                error: model_error,
                                ..Default::default()
                            };
                            local.index_only = None;
                        }
                        Ok(())
                    });
            let proceed = {
                let mut job = state.job.lock().unwrap();
                job.running = false;
                job.paused = state.cancel.load(Ordering::Relaxed);
                match result {
                    Ok(()) => {
                        job.message = if job.paused {
                            "模型安装已暂停"
                        } else {
                            "模型文件已校验并保存"
                        }
                        .into()
                    }
                    Err(error) => job.error = Some(error.to_string()),
                }
                !job.paused && job.error.is_none()
            };
            if proceed {
                if let Err(error) = state.auto_index() {
                    state.job.lock().unwrap().error = Some(error.to_string());
                }
            }
        });
        Ok(())
    }
    pub fn pause_install(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn start_vision(
        self: &Arc<Self>,
        id: Option<&str>,
        action: Option<TaskAction>,
    ) -> Result<()> {
        let mut session = self.vision.lock().unwrap();
        if action == Some(TaskAction::Pause) {
            session.job.paused = true;
            session.job.message = "当前请求完成后暂停".into();
            return Ok(());
        }
        if session.job.running {
            if action == Some(TaskAction::Resume) {
                session.job.paused = false;
                return Ok(());
            }
            return if action.is_some() {
                Err("请先暂停图片理解，等待当前请求完成".into())
            } else {
                Ok(())
            };
        }
        let Some(client) = session.client.clone() else {
            return if action.is_some() {
                Err("请先配置并保存图片理解 API".into())
            } else {
                Ok(())
            };
        };
        if action.is_none() && (session.job.paused || !client.config.auto_describe) {
            return Ok(());
        }
        let library = self.library_write.lock().unwrap();
        if matches!(action, Some(TaskAction::Generate | TaskAction::Retry)) {
            library.queue_task(TaskKind::Vision, id, action == Some(TaskAction::Retry))?;
        }
        let total = if id.is_some() {
            1
        } else {
            library.task_counts(TaskKind::Vision)?.0
        };
        if total == 0 {
            return Ok(());
        }
        session.job = JobStatus {
            running: true,
            total,
            message: "准备生成描述".into(),
            ..Default::default()
        };
        let state = Arc::clone(self);
        let only = id.map(str::to_owned);
        std::thread::spawn(move || {
            if let Err(error) = state.describe_pending(&client, only.as_deref()) {
                let mut session = state.vision.lock().unwrap();
                session.job.paused = true;
                session.job.running = false;
                session.job.error = Some(error.to_string());
                session.job.message = "图片理解已暂停".into();
            }
        });
        Ok(())
    }
    fn describe_pending(
        self: &Arc<Self>,
        client: &VisionClient,
        mut only: Option<&str>,
    ) -> Result<()> {
        loop {
            let asset = {
                let mut session = self.vision.lock().unwrap();
                if session.job.paused {
                    session.job.running = false;
                    session.job.message = "图片理解已暂停".into();
                    return Ok(());
                }
                let library = self.library_read.lock().unwrap();
                let Some(asset) = library.next_vision(only)? else {
                    if only.is_some() && client.config.auto_describe {
                        only = None;
                        continue;
                    }
                    session.job.running = false;
                    session.job.message = "描述生成完成".into();
                    return Ok(());
                };
                session.job.message = asset.name.clone();
                asset
            };
            let result = client.describe(Path::new(&asset.path));
            {
                let library = self.library_write.lock().unwrap();
                if library
                    .find_asset(&asset.id)?
                    .is_some_and(|a| a.ai_state == "pending")
                {
                    match result {
                        Ok(description) => library.update_vision(&asset.id, &description)?,
                        Err(error) => {
                            library.fail_vision(&asset.id, &error.to_string())?;
                            return Err(format!("{}：{error}", asset.name).into());
                        }
                    }
                }
            }
            self.vision.lock().unwrap().job.done += 1;
            self.auto_index()?;
        }
    }
    pub fn import_paths(
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
            let root = self.library_write.lock().unwrap().root.clone();
            for (index, path) in paths.iter().enumerate() {
                let item = (|| -> Result<bool> {
                    let path = Path::new(path);
                    let (id, bytes) = read_source(path)?;
                    if self
                        .library_write
                        .lock()
                        .unwrap()
                        .attach_duplicate(&id, group)?
                    {
                        return Ok(false);
                    }
                    let prepared = PreparedImport::new(&root, path, id, &bytes)?;
                    self.library_write
                        .lock()
                        .unwrap()
                        .insert_import(prepared, group)
                })();
                let added = matches!(item, Ok(true));
                match item {
                    Ok(true) => report.added += 1,
                    Ok(false) => report.duplicates += 1,
                    Err(error) => report.errors.push(format!(
                        "{}：{error}",
                        Path::new(path)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                    )),
                }
                {
                    let mut job = self.import_job.lock().unwrap();
                    job.done = index + 1;
                    job.message = format!("已导入 {} 张", report.added);
                }
                if added {
                    self.auto_index()?;
                    self.start_vision(None, None)?;
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
    pub fn invalidate_searches(&self) -> u64 {
        self.inference_scheduler.invalidate_searches()
    }
    pub fn search(
        &self,
        generation: u64,
        query: &str,
        mode: &str,
        group_id: Option<i64>,
    ) -> Result<Option<Vec<Asset>>> {
        if !self.inference_scheduler.is_search_current(generation) {
            return Ok(None);
        }
        if !["hybrid", "keyword", "semantic"].contains(&mode) {
            return Err("无效的搜索模式".into());
        }
        if query.chars().count() > 4000 {
            return Err("搜索内容过长".into());
        }
        let mut embedded = None;
        if !query.trim().is_empty() && mode != "keyword" {
            let settings = self.library_read.lock().unwrap().settings()?;
            if !settings.semantic_enabled && mode == "semantic" {
                return Err("语义搜索已关闭，请先在偏好设置中启用".into());
            }
            let missing = intelligence::missing_embedding(&self.models, settings.embedding_model);
            if settings.semantic_enabled && !missing.is_empty() {
                if mode == "semantic" {
                    return Err(format!("语义模型不可用：{}", missing.join("、")).into());
                }
            } else if settings.semantic_enabled {
                let Some(_permit) = self.inference_scheduler.acquire_search_permit(generation)
                else {
                    return Ok(None);
                };
                let result = self.with_models(TaskKind::Index, |models| {
                    // Model replacement may have kept this request waiting on the mutex.
                    if !self.inference_scheduler.is_search_current(generation) {
                        return Ok(None);
                    }
                    models.ensure_embedding()?;
                    // Loading a large model can outlast the query that requested it.
                    if !self.inference_scheduler.is_search_current(generation) {
                        return Ok(None);
                    }
                    let vector = models.embed(query, true)?;
                    let version = models
                        .version
                        .as_ref()
                        .map_err(|error| error.clone())?
                        .clone();
                    Ok(Some((vector, version)))
                });
                if !self.inference_scheduler.is_search_current(generation) {
                    return Ok(None);
                }
                embedded = result?.flatten();
                if embedded.is_none() {
                    return Ok(None);
                }
            }
        }
        let data = {
            let library = self.library_read.lock().unwrap();
            if !self.inference_scheduler.is_search_current(generation) {
                return Ok(None);
            }
            // Match the model that produced the query even if settings have changed since inference.
            library.search_data(
                group_id,
                embedded.as_ref().map(|(_, version)| version.as_str()),
            )?
        };
        if !self.inference_scheduler.is_search_current(generation) {
            return Ok(None);
        }
        data.rank(
            query,
            mode,
            embedded.as_ref().map(|(vector, _)| vector.as_slice()),
        )
        .map(Some)
    }

    pub fn model_status(&self) -> Result<ModelStatus> {
        let job = self.job.lock().unwrap().clone();
        let import = self.import_job.lock().unwrap().clone();
        let vision = self.vision.lock().unwrap().job.clone();
        let (ocr, index) = {
            let local = self.local.lock().unwrap();
            (local.ocr.clone(), local.index.clone())
        };
        let library = self.library_read.lock().unwrap();
        let library_revision = library.revision()?;
        let settings = library.settings()?;
        let counts = library.all_task_counts()?;
        drop(library);
        let update = |mut job: JobStatus, counts| {
            (job.pending, job.failed, job.completed, job.incomplete) = counts;
            job.total = job.done + job.pending + job.failed;
            if !job.running {
                job.message.clear();
            }
            if !job.running && job.pending == 0 && job.failed == 0 {
                job.error = None;
            }
            job
        };
        Ok(ModelStatus {
            library_revision,
            ready: intelligence::missing_embedding(&self.models, settings.embedding_model)
                .is_empty(),
            ocr_ready: intelligence::missing_ocr(&self.models).is_empty(),
            ocr_enabled: settings.ocr_enabled,
            semantic_enabled: settings.semantic_enabled,
            directory: self.models.to_string_lossy().into_owned(),
            missing: intelligence::missing(&self.models, settings.embedding_model),
            job,
            import,
            ocr: update(ocr, counts.ocr),
            index: update(index, counts.index),
            vision: update(vision, counts.vision),
        })
    }
    pub fn vision_status(&self) -> Result<VisionStatus> {
        let session = self.vision.lock().unwrap();
        Ok(VisionStatus {
            config: self
                .library_read
                .lock()
                .unwrap()
                .vision_config()?
                .unwrap_or_default(),
            active: session.client.is_some(),
        })
    }
    pub fn save_vision_config(self: &Arc<Self>, config: VisionConfig) -> Result<()> {
        {
            let mut session = self.vision.lock().unwrap();
            if session.job.running {
                return Err("请先暂停图片理解任务再修改配置".into());
            }
            let client = VisionClient::new(config)?;
            self.library_write
                .lock()
                .unwrap()
                .save_vision_config(&client.config)?;
            session.client = Some(client);
        }
        self.start_vision(None, None)
    }
    pub fn test_vision(&self) -> Result<Description> {
        let client = self
            .vision
            .lock()
            .unwrap()
            .client
            .clone()
            .ok_or("请先保存 API 配置")?;
        client.probe()
    }
}
#[cfg(test)]
mod tests;
