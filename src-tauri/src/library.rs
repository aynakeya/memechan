use crate::{import::PreparedImport, intelligence::EmbeddingModel, Result};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const ASSET_COLUMNS: &str = "id,name,extension,width,height,bytes,animated,description,ocr_text,ocr_state,index_state,favorite,created_at,ai_description,ai_state,ai_error,ai_model";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub path: String,
    pub thumbnail: String,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub animated: bool,
    pub description: String,
    pub ai_description: String,
    pub ai_state: String,
    pub ai_error: String,
    pub ai_model: String,
    pub ocr_text: String,
    pub ocr_state: String,
    pub index_state: String,
    pub favorite: bool,
    pub created_at: i64,
    pub groups: Vec<i64>,
    pub score: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub count: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub copy_size: u32,
    pub allow_upscale: bool,
    pub auto_index: bool,
    #[serde(default = "enabled_by_default")]
    pub semantic_enabled: bool,
    #[serde(default = "enabled_by_default")]
    pub ocr_enabled: bool,
    #[serde(default)]
    pub embedding_model: EmbeddingModel,
}
fn enabled_by_default() -> bool {
    true
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            copy_size: 0,
            allow_upscale: false,
            auto_index: true,
            semantic_enabled: true,
            ocr_enabled: true,
            embedding_model: EmbeddingModel::default(),
        }
    }
}
#[derive(Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub assets: Vec<Asset>,
    pub groups: Vec<Group>,
    pub settings: Settings,
}
#[derive(Default, Serialize)]
pub struct ImportReport {
    pub added: usize,
    pub duplicates: usize,
    pub errors: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Ocr,
    Index,
    Vision,
}
impl TaskKind {
    fn column(self) -> &'static str {
        match self {
            Self::Ocr => "ocr_state",
            Self::Index => "index_state",
            Self::Vision => "ai_state",
        }
    }
}

#[derive(Clone, Copy)]
pub struct TaskCounts {
    // Pending, failed, completed, incomplete; idle items are included in incomplete.
    pub ocr: (usize, usize, usize, usize),
    pub index: (usize, usize, usize, usize),
    pub vision: (usize, usize, usize, usize),
}

pub struct Library {
    pub root: PathBuf,
    db: Connection,
    task_counts_cache: Cell<Option<(u64, u64, TaskCounts)>>,
}
impl Library {
    pub fn open(root: &Path) -> Result<Self> {
        for dir in ["originals", "thumbnails", "copies", "models"] {
            fs::create_dir_all(root.join(dir))?;
        }
        let mut db = Connection::open(root.join("library.sqlite3"))?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
          CREATE TABLE IF NOT EXISTS assets (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, extension TEXT NOT NULL,
            width INTEGER NOT NULL, height INTEGER NOT NULL, bytes INTEGER NOT NULL,
            animated INTEGER NOT NULL, description TEXT NOT NULL DEFAULT '',
            ocr_text TEXT NOT NULL DEFAULT '', ocr_state TEXT NOT NULL DEFAULT 'pending',
            index_state TEXT NOT NULL DEFAULT 'pending', favorite INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS groups (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, color TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS asset_groups (asset_id TEXT REFERENCES assets(id) ON DELETE CASCADE,
            group_id INTEGER REFERENCES groups(id) ON DELETE CASCADE, PRIMARY KEY(asset_id,group_id));
          CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS vision_settings (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS index_model (id INTEGER PRIMARY KEY CHECK(id=1), version TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS embeddings (asset_id TEXT PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
            model TEXT NOT NULL, source TEXT NOT NULL, vector BLOB NOT NULL);
          INSERT OR IGNORE INTO index_model SELECT 1,model FROM embeddings LIMIT 1;")?;
        let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version < 2 {
            // Keep existing OCR text searchable until the imported v5 models replace it.
            let tx = db.transaction()?;
            tx.execute_batch(
                "UPDATE assets SET ocr_state='pending',index_state='pending';
              PRAGMA user_version=2;",
            )?;
            tx.commit()?;
        }
        if version < 3 {
            let tx = db.transaction()?;
            tx.execute_batch(
                "ALTER TABLE assets ADD COLUMN ai_description TEXT NOT NULL DEFAULT '';
                ALTER TABLE assets ADD COLUMN ai_state TEXT NOT NULL DEFAULT 'idle';
                ALTER TABLE assets ADD COLUMN ai_error TEXT NOT NULL DEFAULT '';
                ALTER TABLE assets ADD COLUMN ai_model TEXT NOT NULL DEFAULT '';
                PRAGMA user_version=3;",
            )?;
            tx.commit()?;
        }
        let mut library = Self {
            root: root.to_path_buf(),
            db,
            task_counts_cache: Cell::new(None),
        };
        let model = library.settings()?.embedding_model;
        let embedding_version: String = library
            .db
            .query_row("SELECT version FROM index_model WHERE id=1", [], |row| {
                row.get(0)
            })
            .optional()?
            .unwrap_or_else(|| model.version().to_owned());
        // Model changes invalidate only vectors; keep existing OCR text and media.
        let tx = library.db.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO index_model VALUES(1,?1)",
            [&embedding_version],
        )?;
        tx.execute(
            "UPDATE assets SET index_state='pending' WHERE id IN
             (SELECT asset_id FROM embeddings WHERE model<>?1)",
            [&embedding_version],
        )?;
        tx.execute(
            "DELETE FROM embeddings WHERE model<>?1",
            [&embedding_version],
        )?;
        tx.execute_batch(
            "DROP INDEX IF EXISTS assets_pending;
            CREATE INDEX IF NOT EXISTS assets_ocr_pending ON assets(created_at) WHERE ocr_state='pending';
            CREATE INDEX IF NOT EXISTS assets_index_pending ON assets(created_at) WHERE index_state='pending';
            CREATE INDEX IF NOT EXISTS assets_vision_pending ON assets(created_at) WHERE ai_state='pending';",
        )?;
        tx.commit()?;
        Ok(library)
    }
    // Open only after the writable connection has initialized and migrated the database.
    pub fn open_read_only(root: &Path) -> Result<Self> {
        Ok(Self {
            root: root.to_path_buf(),
            db: Connection::open_with_flags(
                root.join("library.sqlite3"),
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?,
            task_counts_cache: Cell::new(None),
        })
    }
    pub fn embedding_version(&self) -> Result<String> {
        Ok(self
            .db
            .query_row("SELECT version FROM index_model WHERE id=1", [], |row| {
                row.get(0)
            })?)
    }
    pub fn settings(&self) -> Result<Settings> {
        let json: Option<String> = self
            .db
            .query_row("SELECT value FROM settings WHERE id=1", [], |row| {
                row.get(0)
            })
            .optional()?;
        match json {
            Some(json) => Ok(serde_json::from_str(&json)?),
            None => Ok(Settings::default()),
        }
    }
    pub fn save_settings(&mut self, settings: Settings) -> Result<()> {
        if settings.copy_size > 4096 {
            return Err("复制最长边不能超过 4096 px".into());
        }
        let changed_model = self.settings()?.embedding_model != settings.embedding_model;
        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT INTO settings VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET value=excluded.value",
            [serde_json::to_string(&settings)?],
        )?;
        if changed_model {
            // Explicitly cleared indexes stay idle until the user rebuilds them.
            tx.execute(
                "UPDATE assets SET index_state='pending' WHERE index_state<>'idle'",
                [],
            )?;
            tx.execute("DELETE FROM embeddings", [])?;
            tx.execute(
                "UPDATE index_model SET version=?1 WHERE id=1",
                [settings.embedding_model.version()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn set_embedding_version(&mut self, version: &str) -> Result<bool> {
        if self.embedding_version()? == version {
            return Ok(false);
        }
        let tx = self.db.transaction()?;
        tx.execute(
            "UPDATE assets SET index_state='pending' WHERE index_state<>'idle'",
            [],
        )?;
        tx.execute("DELETE FROM embeddings", [])?;
        tx.execute("UPDATE index_model SET version=?1 WHERE id=1", [version])?;
        tx.commit()?;
        Ok(true)
    }
    // Compare only values from the same read connection; this tracks other connections' commits.
    pub fn revision(&self) -> Result<u64> {
        Ok(self
            .db
            .pragma_query_value(None, "data_version", |row| row.get(0))?)
    }
    pub fn vision_config(&self) -> Result<Option<crate::vision::VisionConfig>> {
        let json: Option<String> = self
            .db
            .query_row("SELECT value FROM vision_settings WHERE id=1", [], |row| {
                row.get(0)
            })
            .optional()?;
        json.map(|json| serde_json::from_str(&json).map_err(Into::into))
            .transpose()
    }
    pub fn save_vision_config(&self, config: &crate::vision::VisionConfig) -> Result<()> {
        self.db.execute(
            "INSERT INTO vision_settings VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET value=excluded.value",
            [serde_json::to_string(config)?],
        )?;
        Ok(())
    }
    pub fn next_vision(&self, only: Option<&str>) -> Result<Option<Asset>> {
        let tx = self.db.unchecked_transaction()?;
        let id: Option<String> = self
            .db
            .query_row(
            "SELECT id FROM assets WHERE ai_state='pending' AND (?1 IS NULL OR id=?1) ORDER BY created_at,rowid LIMIT 1",
            [only],
                |row| row.get(0),
            )
            .optional()?;
        let asset = id.map(|id| self.asset(&id)).transpose()?;
        tx.commit()?;
        Ok(asset)
    }
    pub fn update_vision(&self, id: &str, description: &crate::vision::Description) -> Result<()> {
        self.db.execute(
            "UPDATE assets SET ai_description=?1,ai_model=?2,ai_state='ready',ai_error='',
             index_state=CASE WHEN index_state='idle' THEN 'idle' ELSE 'pending' END WHERE id=?3",
            params![description.text, description.model, id],
        )?;
        Ok(())
    }
    pub fn fail_vision(&self, id: &str, error: &str) -> Result<()> {
        self.db.execute(
            "UPDATE assets SET ai_state='error',ai_error=?1 WHERE id=?2",
            params![error, id],
        )?;
        Ok(())
    }
    pub fn clear_text(&mut self, kind: TaskKind, id: Option<&str>) -> Result<usize> {
        let update = match kind {
            TaskKind::Ocr => "ocr_text='',ocr_state='idle',index_state=CASE WHEN index_state='idle' THEN 'idle' ELSE 'pending' END",
            TaskKind::Vision => "ai_description='',ai_state='idle',ai_error='',ai_model='',index_state=CASE WHEN index_state='idle' THEN 'idle' ELSE 'pending' END",
            TaskKind::Index => "index_state='idle'",
        };
        if let Some(id) = id {
            self.asset(id)?;
        }
        let tx = self.db.transaction()?;
        let changed = tx.execute(
            &format!("UPDATE assets SET {update} WHERE (?1 IS NULL OR id=?1)"),
            [id],
        )?;
        tx.execute(
            "DELETE FROM embeddings WHERE (?1 IS NULL OR asset_id=?1)",
            [id],
        )?;
        tx.commit()?;
        Ok(changed)
    }
    pub fn queue_task(&self, kind: TaskKind, id: Option<&str>, retry: bool) -> Result<()> {
        if let Some(id) = id {
            self.asset(id)?;
        }
        let column = kind.column();
        let extra = if kind == TaskKind::Vision {
            ",ai_error=''"
        } else {
            ""
        };
        self.db.execute(
            &format!(
                "UPDATE assets SET {column}='pending'{extra} WHERE
            (?1 IS NOT NULL AND id=?1) OR
            (?1 IS NULL AND ({column}='error' OR (?2=0 AND {column}='idle')))"
            ),
            params![id, retry],
        )?;
        Ok(())
    }
    pub fn task_counts(&self, kind: TaskKind) -> Result<(usize, usize, usize, usize)> {
        let counts = self.all_task_counts()?;
        Ok(match kind {
            TaskKind::Ocr => counts.ocr,
            TaskKind::Index => counts.index,
            TaskKind::Vision => counts.vision,
        })
    }
    pub fn all_task_counts(&self) -> Result<TaskCounts> {
        // data_version tracks other connections; total_changes also covers this connection's writes.
        // Capture both before the query so concurrent commits are picked up on the next call.
        let revision = self.revision()?;
        let changes = self.db.total_changes();
        if let Some((cached_revision, cached_changes, counts)) = self.task_counts_cache.get() {
            if cached_revision == revision && cached_changes == changes {
                return Ok(counts);
            }
        }
        let counts = self.db.query_row(
            "SELECT COUNT(*),
             COUNT(*) FILTER (WHERE ocr_state='pending'),
             COUNT(*) FILTER (WHERE ocr_state='error'),
             COUNT(*) FILTER (WHERE ocr_state='ready'),
             COUNT(*) FILTER (WHERE index_state='pending'),
             COUNT(*) FILTER (WHERE index_state='error'),
             COUNT(*) FILTER (WHERE index_state='ready'),
             COUNT(*) FILTER (WHERE ai_state='pending'),
             COUNT(*) FILTER (WHERE ai_state='error'),
             COUNT(*) FILTER (WHERE ai_state='ready') FROM assets",
            [],
            |row| {
                let total: usize = row.get(0)?;
                let read = |offset| -> rusqlite::Result<_> {
                    let completed: usize = row.get(offset + 2)?;
                    Ok((
                        row.get(offset)?,
                        row.get(offset + 1)?,
                        completed,
                        total - completed,
                    ))
                };
                Ok(TaskCounts {
                    ocr: read(1)?,
                    index: read(4)?,
                    vision: read(7)?,
                })
            },
        )?;
        self.task_counts_cache
            .set(Some((revision, changes, counts)));
        Ok(counts)
    }
    pub fn next_task(
        &self,
        kind: TaskKind,
        wait_for_ocr: bool,
        only: Option<&str>,
    ) -> Result<Option<Asset>> {
        let tx = self.db.unchecked_transaction()?;
        let column = kind.column();
        let id: Option<String> = self.db.query_row(&format!(
            "SELECT id FROM assets WHERE {column}='pending' AND (?1=0 OR ocr_state<>'pending') AND (?2 IS NULL OR id=?2) ORDER BY created_at,rowid LIMIT 1"
        ), params![wait_for_ocr, only], |row| row.get(0)).optional()?;
        let asset = id.map(|id| self.asset(&id)).transpose()?;
        tx.commit()?;
        Ok(asset)
    }
    fn read_asset(&self, row: &rusqlite::Row<'_>) -> rusqlite::Result<Asset> {
        let id: String = row.get(0)?;
        let format: String = row.get(2)?;
        Ok(Asset {
            path: self
                .root
                .join("originals")
                .join(format!("{id}.{format}"))
                .to_string_lossy()
                .into_owned(),
            thumbnail: self
                .root
                .join("thumbnails")
                .join(format!("{id}.png"))
                .to_string_lossy()
                .into_owned(),
            id,
            name: row.get(1)?,
            format,
            width: row.get(3)?,
            height: row.get(4)?,
            bytes: row.get(5)?,
            animated: row.get(6)?,
            description: row.get(7)?,
            ocr_text: row.get(8)?,
            ocr_state: row.get(9)?,
            index_state: row.get(10)?,
            favorite: row.get(11)?,
            created_at: row.get(12)?,
            ai_description: row.get(13)?,
            ai_state: row.get(14)?,
            ai_error: row.get(15)?,
            ai_model: row.get(16)?,
            groups: vec![],
            score: None,
        })
    }
    pub fn assets(&self) -> Result<Vec<Asset>> {
        let mut stmt = self.db.prepare(&format!(
            "SELECT {ASSET_COLUMNS} FROM assets ORDER BY created_at DESC,rowid DESC"
        ))?;
        let mut assets = stmt
            .query_map([], |row| self.read_asset(row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut memberships = self
            .db
            .prepare("SELECT asset_id,group_id FROM asset_groups")?;
        let positions: HashMap<_, _> = assets
            .iter()
            .enumerate()
            .map(|(index, asset)| (asset.id.clone(), index))
            .collect();
        for pair in memberships.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })? {
            let (id, group) = pair?;
            if let Some(&index) = positions.get(&id) {
                assets[index].groups.push(group);
            }
        }
        Ok(assets)
    }
    pub fn asset(&self, id: &str) -> Result<Asset> {
        self.find_asset(id)?.ok_or_else(|| "表情不存在".into())
    }
    pub fn find_asset(&self, id: &str) -> Result<Option<Asset>> {
        let asset = self
            .db
            .query_row(
                &format!("SELECT {ASSET_COLUMNS} FROM assets WHERE id=?1"),
                [id],
                |row| self.read_asset(row),
            )
            .optional()?;
        if let Some(mut asset) = asset {
            asset.groups = self
                .db
                .prepare("SELECT group_id FROM asset_groups WHERE asset_id=?1")?
                .query_map([id], |row| row.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            Ok(Some(asset))
        } else {
            Ok(None)
        }
    }
    pub fn groups(&self) -> Result<Vec<Group>> {
        Ok(self.db.prepare("SELECT g.id,g.name,g.color,COUNT(ag.asset_id) FROM groups g LEFT JOIN asset_groups ag ON ag.group_id=g.id GROUP BY g.id ORDER BY g.id")?
          .query_map([], |row| Ok(Group { id: row.get(0)?, name: row.get(1)?, color: row.get(2)?, count: row.get(3)? }))?
          .collect::<std::result::Result<Vec<_>, _>>()?)
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        // Read the revision first so commits after it cannot be missed by the next poll.
        let revision = self.revision()?;
        let tx = self.db.unchecked_transaction()?;
        let snapshot = Snapshot {
            revision,
            assets: self.assets()?,
            groups: self.groups()?,
            settings: self.settings()?,
        };
        tx.commit()?;
        Ok(snapshot)
    }
    pub fn save_group(&self, id: Option<i64>, name: &str, color: &str) -> Result<Group> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err("分组名称应为 1–40 个字符".into());
        }
        if color.len() != 7
            || !color.starts_with('#')
            || !color[1..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("无效的分组颜色".into());
        }
        let id = match id {
            Some(id) => {
                if self.db.execute(
                    "UPDATE groups SET name=?1,color=?2 WHERE id=?3",
                    params![name, color, id],
                )? == 0
                {
                    return Err("分组不存在".into());
                }
                id
            }
            None => {
                self.db.execute(
                    "INSERT INTO groups(name,color) VALUES(?1,?2)",
                    params![name, color],
                )?;
                self.db.last_insert_rowid()
            }
        };
        Ok(self
            .groups()?
            .into_iter()
            .find(|group| group.id == id)
            .ok_or("分组不存在")?)
    }
    pub fn delete_group(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM groups WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn save_asset(
        &mut self,
        id: &str,
        description: &str,
        favorite: bool,
        groups: &[i64],
    ) -> Result<()> {
        if description.chars().count() > 4000 {
            return Err("描述不能超过 4000 个字符".into());
        }
        let tx = self.db.transaction()?;
        let changed = tx.execute(
            "UPDATE assets SET
               index_state=CASE WHEN description<>?1 AND index_state<>'idle' THEN 'pending' ELSE index_state END,
               description=?1, favorite=?2 WHERE id=?3",
            params![description.trim(), favorite, id],
        )?;
        if changed == 0 {
            return Err("表情不存在".into());
        }
        tx.execute("DELETE FROM asset_groups WHERE asset_id=?1", [id])?;
        for group in groups {
            tx.execute(
                "INSERT OR IGNORE INTO asset_groups VALUES(?1,?2)",
                params![id, group],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn set_assets_group(&mut self, ids: &[String], group: i64, add: bool) -> Result<()> {
        let tx = self.db.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM groups WHERE id=?1)",
            [group],
            |row| row.get(0),
        )?;
        if !exists {
            return Err("分组不存在".into());
        }
        for id in ids {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM assets WHERE id=?1)",
                [id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err("表情不存在".into());
            }
            tx.execute(
                if add {
                    "INSERT OR IGNORE INTO asset_groups VALUES(?1,?2)"
                } else {
                    "DELETE FROM asset_groups WHERE asset_id=?1 AND group_id=?2"
                },
                params![id, group],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn delete_asset(&mut self, id: &str) -> Result<()> {
        self.delete_assets(&[id.to_owned()])
    }
    pub fn delete_assets(&mut self, ids: &[String]) -> Result<()> {
        let assets = ids
            .iter()
            .map(|id| self.asset(id))
            .collect::<Result<Vec<_>>>()?;
        let tx = self.db.transaction()?;
        for id in ids {
            tx.execute("DELETE FROM assets WHERE id=?1", [id])?;
        }
        tx.commit()?;
        // Copy artifacts remain valid for consumers that have not pasted yet.
        for asset in assets {
            for path in [&asset.path, &asset.thumbnail] {
                if Path::new(path).exists() {
                    fs::remove_file(path)?;
                }
            }
        }
        Ok(())
    }
    pub fn attach_duplicate(&self, id: &str, group: Option<i64>) -> Result<bool> {
        let exists: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM assets WHERE id=?1)",
            [id],
            |row| row.get(0),
        )?;
        if exists {
            if let Some(group) = group {
                self.db.execute(
                    "INSERT OR IGNORE INTO asset_groups VALUES(?1,?2)",
                    params![id, group],
                )?;
            }
        }
        Ok(exists)
    }
    pub fn insert_import(&mut self, prepared: PreparedImport, group: Option<i64>) -> Result<bool> {
        let id = &prepared.id;
        if self.attach_duplicate(id, group)? {
            return Ok(false);
        }
        let original = self
            .root
            .join("originals")
            .join(format!("{id}.{}", prepared.format));
        let thumbnail = self.root.join("thumbnails").join(format!("{id}.png"));
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
        let auto_describe = self
            .vision_config()?
            .is_some_and(|config| config.auto_describe);
        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT INTO assets(id,name,extension,width,height,bytes,animated,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                id,
                prepared.name,
                prepared.format,
                prepared.width,
                prepared.height,
                prepared.bytes as u64,
                prepared.animated,
                now
            ],
        )?;
        if let Some(group) = group {
            tx.execute("INSERT INTO asset_groups VALUES(?1,?2)", params![id, group])?;
        }
        if auto_describe {
            tx.execute("UPDATE assets SET ai_state='pending' WHERE id=?1", [&id])?;
        }
        let result = (|| -> Result<()> {
            prepared.original.persist(&original)?;
            prepared.thumbnail.persist(&thumbnail)?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(error) = result {
            for file in [&original, &thumbnail] {
                if file.exists() {
                    fs::remove_file(file)?;
                }
            }
            return Err(error);
        }
        Ok(true)
    }
    pub fn update_ocr(&self, id: &str, text: &str) -> Result<()> {
        self.db.execute(
            "UPDATE assets SET ocr_text=?1,ocr_state='ready',
             index_state=CASE WHEN index_state='idle' THEN 'idle' ELSE 'pending' END WHERE id=?2",
            params![text, id],
        )?;
        Ok(())
    }
    pub fn mark_error(&self, id: &str, ocr: bool) -> Result<()> {
        self.db.execute(
            if ocr {
                "UPDATE assets SET ocr_state='error' WHERE id=?1"
            } else {
                "UPDATE assets SET index_state='error' WHERE id=?1"
            },
            [id],
        )?;
        Ok(())
    }
    pub fn put_embedding(&mut self, id: &str, source: &str, vector: &[f32]) -> Result<()> {
        let Some(current) = self.find_asset(id)? else {
            return Ok(());
        };
        if current.index_state == "idle" || Self::search_text(&current) != source {
            return Ok(());
        }
        let bytes: Vec<u8> = vector
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let version = self.embedding_version()?;
        let tx = self.db.transaction()?;
        tx.execute("INSERT INTO embeddings VALUES(?1,?2,?3,?4) ON CONFLICT(asset_id) DO UPDATE SET model=excluded.model,source=excluded.source,vector=excluded.vector",
          params![id,version,source,bytes])?;
        tx.execute("UPDATE assets SET index_state='ready' WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn search_text(asset: &Asset) -> String {
        let mut text = format!("{}\n{}\n{}", asset.name, asset.description, asset.ocr_text);
        // Preserve existing vectors for assets without an API description.
        if !asset.ai_description.is_empty() {
            text.push('\n');
            text.push_str(&asset.ai_description);
        }
        text
    }
    // Return owned data so callers can release the read lock before matching and ranking.
    pub fn search_data(&self, group: Option<i64>, version: Option<&str>) -> Result<SearchData> {
        let tx = self.db.unchecked_transaction()?;
        let assets = self
            .assets()?
            .into_iter()
            .filter(|asset| group.is_none_or(|id| asset.groups.contains(&id)))
            .collect();
        let embeddings = if let Some(version) = version {
            self.db
                .prepare(
                    "SELECT asset_id,source,vector FROM embeddings WHERE model=?1 AND
                (?2 IS NULL OR asset_id IN (SELECT asset_id FROM asset_groups WHERE group_id=?2))",
                )?
                .query_map(params![version, group], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        tx.commit()?;
        Ok(SearchData { assets, embeddings })
    }
}

pub struct SearchData {
    assets: Vec<Asset>,
    embeddings: Vec<(String, String, Vec<u8>)>,
}
impl SearchData {
    pub fn rank(self, query: &str, mode: &str, embedding: Option<&[f32]>) -> Result<Vec<Asset>> {
        if !["hybrid", "keyword", "semantic"].contains(&mode) {
            return Err("无效的搜索模式".into());
        }
        let query = query.trim().to_lowercase();
        let mut assets = self.assets;
        if query.is_empty() {
            return Ok(assets);
        }
        let sources: Vec<_> = assets.iter().map(Library::search_text).collect();
        let mut keyword = vec![];
        let mut semantic = vec![];
        for (index, source) in sources.iter().enumerate() {
            if mode != "semantic" && source.to_lowercase().contains(&query) {
                keyword.push(index);
            }
        }
        if let Some(query) = embedding.filter(|_| mode != "keyword") {
            let positions: HashMap<_, _> = assets
                .iter()
                .enumerate()
                .map(|(index, asset)| (asset.id.as_str(), index))
                .collect();
            for (id, source, bytes) in self.embeddings {
                let Some(&index) = positions.get(id.as_str()) else {
                    continue;
                };
                if source != sources[index] || bytes.len() != query.len() * 4 {
                    continue;
                }
                let score = query
                    .iter()
                    .zip(bytes.chunks_exact(4))
                    .map(|(query, bytes)| query * f32::from_le_bytes(bytes.try_into().unwrap()))
                    .sum::<f32>();
                semantic.push((index, score));
            }
        }
        semantic.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut scores = HashMap::<usize, f32>::new();
        if mode != "semantic" {
            for (rank, index) in keyword.into_iter().enumerate() {
                *scores.entry(index).or_default() += 1.0 / (60.0 + rank as f32);
            }
        }
        if mode != "keyword" {
            for (rank, (index, _)) in semantic.into_iter().take(60).enumerate() {
                *scores.entry(index).or_default() += 1.0 / (60.0 + rank as f32);
            }
        }
        for (index, asset) in assets.iter_mut().enumerate() {
            asset.score = scores.get(&index).copied();
        }
        assets.retain(|a| a.score.is_some());
        assets.sort_by(|a, b| b.score.unwrap().total_cmp(&a.score.unwrap()));
        assets.truncate(100);
        Ok(assets)
    }
}

#[cfg(test)]
mod tests;
