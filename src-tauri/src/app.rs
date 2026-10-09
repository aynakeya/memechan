use crate::{
    intelligence::{self, Intelligence},
    library::{Asset, ImportReport, Library},
    vision::{Description, VisionClient, VisionConfig},
    Result,
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

pub struct AppState {
    pub(crate) library: Mutex<Library>,
    models: PathBuf,
    intelligence: Mutex<Option<Intelligence>>,
    job: Mutex<JobStatus>,
    import_job: Mutex<JobStatus>,
    cancel: AtomicBool,
    vision: Mutex<VisionSession>,
}
#[derive(Default)]
struct VisionSession {
    client: Option<VisionClient>,
    job: JobStatus,
    paused: bool,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionStatus {
    config: VisionConfig,
    active: bool,
    has_api_key: bool,
}
#[derive(Clone, Default, Serialize)]
pub struct JobStatus {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub message: String,
    pub error: Option<String>,
}
#[derive(Serialize)]
pub struct ModelStatus {
    pub ready: bool,
    pub directory: String,
    pub missing: Vec<String>,
    pub job: JobStatus,
    pub import: JobStatus,
    pub vision: JobStatus,
}

pub enum ModelInstall {
    Download,
    Local(Vec<PathBuf>),
}
impl AppState {
    pub fn new(root: &Path) -> Result<Self> {
        Ok(Self {
            library: Mutex::new(Library::open(root)?),
            models: root.join("models"),
            intelligence: Mutex::new(None),
            job: Mutex::new(JobStatus::default()),
            import_job: Mutex::new(JobStatus::default()),
            cancel: AtomicBool::new(false),
            vision: Mutex::new(VisionSession::default()),
        })
    }
    pub fn start_vision(self: &Arc<Self>, id: Option<&str>, explicit: bool) -> Result<()> {
        let mut session = self.vision.lock().unwrap();
        if session.job.running {
            return if explicit {
                Err("图片理解任务正在运行".into())
            } else {
                Ok(())
            };
        }
        let Some(client) = session.client.clone() else {
            return if explicit {
                Err("请先配置并保存图片理解 API".into())
            } else {
                Ok(())
            };
        };
        if !explicit && (session.paused || !client.config.auto_describe) {
            return Ok(());
        }
        let total = {
            let library = self.library.lock().unwrap();
            if explicit {
                library.queue_vision(id)?;
            }
            library.vision_pending_count()?
        };
        if total == 0 {
            return Ok(());
        }
        session.paused = false;
        session.job = JobStatus {
            running: true,
            total,
            message: "准备生成描述".into(),
            ..Default::default()
        };
        let state = Arc::clone(self);
        let only = id.map(str::to_owned);
        std::thread::spawn(move || {
            let result = state.describe_pending(&client, only.as_deref());
            if let Err(error) = result {
                let mut session = state.vision.lock().unwrap();
                session.paused = true;
                session.job.running = false;
                session.job.error = Some(error.to_string());
                session.job.message = "图片理解已暂停，请检查错误后重试".into();
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
                // Queue completion and worker starts share a lock, including new imports.
                let mut session = self.vision.lock().unwrap();
                if session.paused {
                    session.job.running = false;
                    session.job.message = "图片理解已暂停".into();
                    return Ok(());
                }
                let library = self.library.lock().unwrap();
                let Some(asset) = library.next_vision(only)? else {
                    if only.is_some() && client.config.auto_describe {
                        only = None;
                        continue;
                    }
                    session.job.running = false;
                    session.job.message = "描述生成完成".into();
                    return Ok(());
                };
                session.job.total = session.job.done
                    + if only.is_some() {
                        1
                    } else {
                        library.vision_pending_count()?
                    };
                session.job.message = format!("生成描述 · {}", asset.name);
                asset
            };
            // No database or model locks are held across the network request.
            match client.describe(Path::new(&asset.path)) {
                Ok(description) => {
                    self.library
                        .lock()
                        .unwrap()
                        .update_vision(&asset.id, &description)?;
                    self.auto_index()?;
                }
                Err(error) => {
                    self.library
                        .lock()
                        .unwrap()
                        .fail_vision(&asset.id, &error.to_string())?;
                    return Err(format!("{}：{error}", asset.name).into());
                }
            }
            self.vision.lock().unwrap().job.done += 1;
        }
    }
    fn with_models<T>(&self, work: impl FnOnce(&mut Intelligence) -> Result<T>) -> Result<T> {
        let mut models = self.intelligence.lock().unwrap();
        if models.is_none() {
            *models = Some(Intelligence::load(&self.models)?);
        }
        work(models.as_mut().unwrap())
    }
    pub fn auto_index(self: &Arc<Self>) -> Result<()> {
        let enabled = self.library.lock().unwrap().settings()?.auto_index;
        if enabled
            && !self.cancel.load(Ordering::Relaxed)
            && intelligence::missing(&self.models).is_empty()
        {
            self.start_job(None, None, false)?;
        }
        Ok(())
    }
    pub fn start_job(
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
        if id.is_some() || (explicit && install.is_none()) {
            self.library.lock().unwrap().queue_index(id.as_deref())?;
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
                    self.start_vision(None, false)?;
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

impl AppState {
    pub fn search(&self, query: &str, mode: &str, group_id: Option<i64>) -> Result<Vec<Asset>> {
        if !["hybrid", "keyword", "semantic"].contains(&mode) {
            return Err("无效的搜索模式".into());
        }
        if query.chars().count() > 4000 {
            return Err("搜索内容过长".into());
        }
        let mut embedding = None;
        if !query.trim().is_empty() && mode != "keyword" {
            if intelligence::missing(&self.models).is_empty() {
                embedding = Some(self.with_models(|models| models.embed(query, true))?);
            } else if mode == "semantic" {
                return Err("语义搜索需要本地模型，请先导入或下载模型并建立索引".into());
            }
        }
        self.library
            .lock()
            .unwrap()
            .search(query, mode, group_id, embedding.as_deref())
    }
    pub fn model_status(&self) -> ModelStatus {
        let missing = intelligence::missing(&self.models);
        ModelStatus {
            ready: missing.is_empty(),
            directory: self.models.to_string_lossy().into_owned(),
            missing,
            job: self.job.lock().unwrap().clone(),
            import: self.import_job.lock().unwrap().clone(),
            vision: self.vision.lock().unwrap().job.clone(),
        }
    }
    pub fn vision_status(&self) -> Result<VisionStatus> {
        let session = self.vision.lock().unwrap();
        Ok(VisionStatus {
            config: self.library.lock().unwrap().vision_config()?,
            active: session.client.is_some(),
            has_api_key: session.client.as_ref().is_some_and(VisionClient::has_key),
        })
    }
    pub fn save_vision_config(
        self: &Arc<Self>,
        config: VisionConfig,
        api_key: Option<String>,
    ) -> Result<()> {
        {
            let mut session = self.vision.lock().unwrap();
            if session.job.running {
                return Err("请先暂停图片理解任务再修改配置".into());
            }
            let key = match api_key {
                Some(key) => key,
                None => match &session.client {
                    Some(client) => client.retained_key(&config)?,
                    None => String::new(),
                },
            };
            let client = VisionClient::new(config.clone(), key)?;
            self.library.lock().unwrap().save_vision_config(&config)?;
            session.client = Some(client);
        }
        self.start_vision(None, false)
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
    pub fn pause_vision(&self) {
        let mut session = self.vision.lock().unwrap();
        session.paused = true;
        if session.job.running {
            session.job.message = "当前请求完成后暂停".into();
        }
    }
    pub fn pause_index(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests;
