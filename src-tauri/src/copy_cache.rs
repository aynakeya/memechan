use crate::Result;
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

pub const LIMIT_BYTES: u64 = 512 * 1024 * 1024;
pub const RECENT: Duration = Duration::from_secs(10 * 60);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheUsage {
    pub bytes: u64,
    pub files: usize,
    pub limit_bytes: u64,
    pub error: Option<String>,
}

pub struct CopyCache {
    root: PathBuf,
    cleanup_error: Option<String>,
}

impl CopyCache {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            cleanup_error: None,
        }
    }

    pub fn usage(&self) -> Result<CacheUsage> {
        let mut usage = usage_of(&entries(&self.root)?);
        usage.error = self.cleanup_error.clone();
        Ok(usage)
    }

    pub fn clean(
        &mut self,
        target_bytes: u64,
        protected_files: impl FnOnce() -> Result<Vec<PathBuf>>,
    ) -> Result<CacheUsage> {
        let result = (|| -> Result<CacheUsage> {
            let mut usage = self.usage()?;
            usage.error = None;
            if usage.files == 0 || (target_bytes > 0 && usage.bytes <= target_bytes) {
                return Ok(usage);
            }
            let protected =
                protected_files().map_err(|error| format!("{error}，已跳过缓存清理"))?;
            clean(&self.root, &protected, target_bytes)
        })();
        self.cleanup_error = result.as_ref().err().map(|error| error.to_string());
        result
    }
}

struct Entry {
    path: PathBuf,
    bytes: u64,
    used: SystemTime,
}

fn entries(root: &Path) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(root.join("copies"))? {
        let entry = entry?;
        // Never follow links or recurse out of the managed cache directory.
        if entry.file_type()?.is_file() {
            let metadata = entry.metadata()?;
            entries.push(Entry {
                path: entry.path(),
                bytes: metadata.len(),
                used: metadata.modified()?,
            });
        }
    }
    Ok(entries)
}

fn usage_of(entries: &[Entry]) -> CacheUsage {
    CacheUsage {
        bytes: entries.iter().map(|entry| entry.bytes).sum(),
        files: entries.len(),
        limit_bytes: LIMIT_BYTES,
        error: None,
    }
}

fn clean(root: &Path, protected: &[PathBuf], target_bytes: u64) -> Result<CacheUsage> {
    let mut entries = entries(root)?;
    let mut usage = usage_of(&entries);
    entries.sort_unstable_by_key(|entry| entry.used);
    let now = SystemTime::now();
    for entry in entries {
        if target_bytes > 0 && usage.bytes <= target_bytes {
            break;
        }
        // Future timestamps also stay protected after a system clock adjustment.
        if protected.contains(&entry.path)
            || !now
                .duration_since(entry.used)
                .is_ok_and(|age| age >= RECENT)
        {
            continue;
        }
        fs::remove_file(&entry.path)?;
        usage.bytes -= entry.bytes;
        usage.files -= 1;
    }
    Ok(usage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn cached(root: &Path, name: &str, age: Duration) -> Result<PathBuf> {
        let path = root.join("copies").join(name);
        fs::write(&path, [0; 10])?;
        File::options()
            .write(true)
            .open(&path)?
            .set_modified(SystemTime::now() - age)?;
        Ok(path)
    }

    #[test]
    fn evicts_oldest_but_keeps_clipboard_and_recent_files() -> Result<()> {
        let root = tempfile::tempdir()?;
        fs::create_dir(root.path().join("copies"))?;
        let oldest = cached(root.path(), "old.png", RECENT * 4)?;
        let active = cached(root.path(), "clipboard.gif", RECENT * 3)?;
        let newer = cached(root.path(), "newer.png", RECENT * 2)?;
        let recent = cached(root.path(), "recent.png", Duration::ZERO)?;
        let future = cached(root.path(), "future.png", Duration::ZERO)?;
        File::options()
            .write(true)
            .open(&future)?
            .set_modified(SystemTime::now() + RECENT)?;
        let mut cache = CopyCache::new(root.path().to_path_buf());
        assert_eq!(cache.usage()?.bytes, 50);
        assert!(cache.clean(40, || Err("剪贴板不可读".into())).is_err());
        let status = cache.usage()?;
        assert_eq!(status.bytes, 50);
        assert_eq!(
            status.error.as_deref(),
            Some("剪贴板不可读，已跳过缓存清理")
        );

        let usage = cache.clean(40, || Ok(vec![active.clone()]))?;
        assert_eq!((usage.bytes, usage.files), (40, 4));
        assert!(cache.usage()?.error.is_none());
        assert!(!oldest.exists());
        assert!(newer.exists());

        let usage = cache.clean(0, || Ok(vec![active.clone()]))?;
        assert_eq!((usage.bytes, usage.files), (30, 3));
        assert!(!newer.exists());
        for protected in [active, recent, future] {
            assert!(protected.exists());
        }
        // Manual cleanup also removes empty files left by an interrupted write.
        let empty = root.path().join("copies/interrupted.part");
        File::create(&empty)?.set_modified(SystemTime::now() - RECENT * 2)?;
        assert_eq!(clean(root.path(), &[], 0)?.files, 2);
        assert!(!empty.exists());
        Ok(())
    }

    #[test]
    fn reusing_a_copy_refreshes_recency_without_changing_its_contents() -> Result<()> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source.png");
        image::DynamicImage::new_rgb8(8, 8).save(&source)?;
        let mut library = crate::library::Library::open(root.path())?;
        library.import_files(&[source.to_string_lossy().into_owned()], None);
        let asset = library.assets()?.remove(0);
        let copy = crate::media::prepare_copy(&asset, root.path(), 0, false)?;
        let contents = fs::read(&copy)?;
        File::options()
            .write(true)
            .open(&copy)?
            .set_modified(SystemTime::now() - RECENT * 2)?;
        assert_eq!(
            crate::media::prepare_copy(&asset, root.path(), 0, false)?,
            copy
        );
        assert_eq!(clean(root.path(), &[], 0)?.files, 1);
        assert_eq!(fs::read(&copy)?, contents);
        assert_eq!(fs::read(&asset.path)?, contents);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_does_not_follow_symlinks_or_subdirectories() -> Result<()> {
        let root = tempfile::tempdir()?;
        fs::create_dir(root.path().join("copies"))?;
        let outside = root.path().join("original.png");
        fs::write(&outside, b"original")?;
        let nested = root.path().join("copies/nested");
        fs::create_dir(&nested)?;
        fs::write(nested.join("keep.png"), b"nested")?;
        std::os::unix::fs::symlink(&outside, root.path().join("copies/link.png"))?;
        assert_eq!(clean(root.path(), &[], 0)?.files, 0);
        assert_eq!(fs::read(&outside)?, b"original");
        assert_eq!(fs::read(nested.join("keep.png"))?, b"nested");
        Ok(())
    }
}
