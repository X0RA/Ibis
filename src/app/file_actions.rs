use std::path::{Path, PathBuf};

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub type TrashUndo = Vec<trash::TrashItem>;
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub type TrashUndo = Vec<PathBuf>;

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn trash_file(path: &Path) -> Result<TrashUndo, String> {
    let before = trash::os_limited::list().map_err(|e| e.to_string())?;
    trash::delete(path).map_err(|e| e.to_string())?;
    let after = trash::os_limited::list().map_err(|e| e.to_string())?;
    Ok(after
        .into_iter()
        .filter(|item| item.original_path() == path && !before.iter().any(|old| old.id == item.id))
        .collect())
}
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn trash_file(path: &Path) -> Result<TrashUndo, String> {
    trash::delete(path).map_err(|e| e.to_string())?;
    Ok(vec![])
}
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn undo(items: TrashUndo) -> Result<(), String> {
    trash::os_limited::restore_all(items).map_err(|e| e.to_string())
}
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn undo(_: TrashUndo) -> Result<(), String> {
    Err("Use your system Trash to restore files on this platform".into())
}

/// No-clobber move, including across filesystems. Originals survive a failed copy.
pub fn move_file(source: &Path, folder: &Path) -> Result<PathBuf, String> {
    let target = folder.join(source.file_name().ok_or("Missing filename")?);
    if source == target {
        return Err("Source and destination are the same".into());
    }
    let mut input = std::fs::File::open(source).map_err(|e| e.to_string())?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|e| e.to_string())?;
    if let Err(error) = std::io::copy(&mut input, &mut output).and_then(|_| output.sync_all()) {
        let _ = std::fs::remove_file(&target);
        return Err(error.to_string());
    }
    let metadata = input.metadata().map_err(|e| e.to_string())?;
    output
        .set_permissions(metadata.permissions())
        .map_err(|e| e.to_string())?;
    let mut times = std::fs::FileTimes::new();
    if let Ok(time) = metadata.modified() {
        times = times.set_modified(time);
    }
    if let Ok(time) = metadata.accessed() {
        times = times.set_accessed(time);
    }
    output.set_times(times).map_err(|e| e.to_string())?;
    std::fs::remove_file(source).map_err(|e| e.to_string())?;
    Ok(target)
}
pub fn reveal(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let uri = url::Url::from_file_path(path).map_err(|_| "Invalid file path")?;
        let result = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--type=method_call",
                "--print-reply",
                "--reply-timeout=2000",
                "--dest=org.freedesktop.FileManager1",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(format!("array:string:{uri}"))
            .arg("string:")
            .status();
        if result.is_ok_and(|s| s.success()) {
            return Ok(());
        }
        std::process::Command::new("xdg-open")
            .arg(path.parent().unwrap_or(path))
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn move_never_overwrites() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("out");
        std::fs::create_dir(&out).unwrap();
        let src = d.path().join("a.jpg");
        std::fs::write(&src, b"source").unwrap();
        std::fs::write(out.join("a.jpg"), b"existing").unwrap();
        assert!(move_file(&src, &out).is_err());
        assert_eq!(std::fs::read(&src).unwrap(), b"source");
        assert_eq!(std::fs::read(out.join("a.jpg")).unwrap(), b"existing");
    }
    #[test]
    fn move_preserves_content_and_timestamp() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("out");
        std::fs::create_dir(&out).unwrap();
        let source = d.path().join("photo.jpg");
        std::fs::write(&source, b"original bytes").unwrap();
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        std::fs::File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(time))
            .unwrap();
        let target = move_file(&source, &out).unwrap();
        assert!(!source.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"original bytes");
        assert_eq!(std::fs::metadata(target).unwrap().modified().unwrap(), time);
    }
}
