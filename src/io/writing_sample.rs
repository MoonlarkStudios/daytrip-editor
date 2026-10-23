//! Each sample opens as an independent, editable local document.

use crate::io::storage::{self, FileError, LoadedFile};

pub async fn load() -> Result<LoadedFile, FileError> {
    let root = storage::preferences_path()
        .and_then(|path| path.parent().map(|p| p.to_owned()))
        .ok_or_else(|| FileError::Io("No application data directory is available.".into()))?;
    let path = tokio::task::spawn_blocking(move || {
        let folder = root.join("samples").join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(folder.join("the-quiet-hour.assets"))?;
        let path = folder.join("The quiet hour.md");
        storage::atomic_write(
            &folder.join("the-quiet-hour.assets/coast.png"),
            include_bytes!("../../assets/writing/coast.png"),
        )?;
        storage::atomic_write(
            &path,
            include_bytes!("../../assets/writing/the-quiet-hour.md"),
        )?;
        Ok::<_, FileError>(path)
    })
    .await
    .map_err(|e| FileError::Io(e.to_string()))??;
    storage::load(path).await
}
