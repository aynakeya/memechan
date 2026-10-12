use crate::Result;
use rusqlite::{Connection, OpenFlags};
use std::{fs, path::Path, time::Duration};

pub fn migrate(legacy: &Path, destination: &Path) -> Result<()> {
    if !legacy.try_exists()? {
        return Ok(());
    }
    if destination.try_exists()? {
        return Err(format!(
            "新旧数据目录同时存在，无法自动迁移，未覆盖任何数据：{} → {}",
            legacy.display(),
            destination.display()
        )
        .into());
    }

    let database = legacy.join("library.sqlite3");
    if database.try_exists()? {
        let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        connection.busy_timeout(Duration::ZERO)?;
        connection
            .execute_batch("PRAGMA locking_mode=EXCLUSIVE; BEGIN EXCLUSIVE; COMMIT;")
            .map_err(|error| {
                format!("无法独占旧资料库，请完全退出旧版 Memechan 后重试：{error}")
            })?;
        // Close SQLite before moving its files, including on Windows.
        connection.close().map_err(|(_, error)| error)?;
    }

    // Both directories share the platform's data root. Rename the whole directory
    // before creating a WebView or opening the library; no partial file-by-file move.
    fs::rename(legacy, destination).map_err(|error| {
        format!(
            "无法迁移数据目录 {} → {}：{error}",
            legacy.display(),
            destination.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Library;

    #[test]
    fn migration_waits_for_library_close_and_preserves_data() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let legacy = directory.path().join("legacy");
        let destination = directory.path().join("current");
        let library = Library::open(&legacy)?;
        library.save_group(None, "我的收藏", "#112233")?;
        for folder in ["originals", "thumbnails", "copies", "models"] {
            fs::write(legacy.join(folder).join("fixture"), folder)?;
        }
        assert!(migrate(&legacy, &destination).is_err());
        assert!(!destination.exists());
        assert_eq!(library.snapshot()?.groups[0].name, "我的收藏");
        drop(library);

        migrate(&legacy, &destination)?;
        assert!(!legacy.exists());
        let migrated = Library::open(&destination)?;
        assert_eq!(migrated.snapshot()?.groups[0].name, "我的收藏");
        for folder in ["originals", "thumbnails", "copies", "models"] {
            assert_eq!(
                fs::read_to_string(destination.join(folder).join("fixture"))?,
                folder
            );
        }
        // Subsequent launches leave the current, open library alone.
        migrate(&legacy, &destination)?;
        Ok(())
    }

    #[test]
    fn migration_does_not_overwrite_an_existing_directory() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let legacy = directory.path().join("legacy");
        let destination = directory.path().join("current");
        migrate(&legacy, &destination)?;
        assert!(!destination.exists());
        for path in [&legacy, &destination] {
            fs::create_dir(path)?;
            fs::write(
                path.join("fixture"),
                path.file_name().unwrap().as_encoded_bytes(),
            )?;
        }
        assert!(migrate(&legacy, &destination).is_err());
        assert_eq!(fs::read_to_string(legacy.join("fixture"))?, "legacy");
        assert_eq!(fs::read_to_string(destination.join("fixture"))?, "current");
        Ok(())
    }
}
