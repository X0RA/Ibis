use std::path::PathBuf;
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}
pub fn cache_base() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return path.into();
    }
    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
    }
    #[cfg(target_os = "macos")]
    {
        return home().join("Library/Caches");
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        home().join(".cache")
    }
}
pub fn data_base() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        return path.into();
    }
    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
    }
    #[cfg(target_os = "macos")]
    {
        return home().join("Library/Application Support");
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        home().join(".local/share")
    }
}
pub fn cache_dir() -> PathBuf {
    app_dir(cache_base())
}
pub fn data_dir() -> PathBuf {
    app_dir(data_base())
}

pub fn config_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return app_dir(PathBuf::from(path));
    }
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("AppData/Roaming"));
    #[cfg(target_os = "macos")]
    let base = home().join("Library/Application Support");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = home().join(".config");
    app_dir(base)
}

// Keep existing settings, ratings and thumbnails when upgrading to ibis.
// The former name is used only for this on-disk compatibility migration.
fn app_dir(base: PathBuf) -> PathBuf {
    let current = base.join("ibis");
    let legacy = base.join("picture_viewer");
    if !current.exists()
        && legacy.is_dir()
        && let Err(error) = std::fs::rename(&legacy, &current)
    {
        // Another worker may already have completed the move. Otherwise retain
        // access to the user's records rather than silently starting afresh.
        if !current.exists() {
            eprintln!("Could not migrate app data to ibis: {error}; using the previous location");
            return legacy;
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_preserves_existing_records_and_never_overwrites_ibis_data() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("picture_viewer");
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join("settings.json"), "saved preference").unwrap();
        let current = app_dir(dir.path().to_path_buf());
        assert_eq!(current, dir.path().join("ibis"));
        assert_eq!(
            std::fs::read_to_string(current.join("settings.json")).unwrap(),
            "saved preference"
        );
        assert!(!legacy.exists());
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join("settings.json"), "older preference").unwrap();
        assert_eq!(app_dir(dir.path().to_path_buf()), current);
        assert_eq!(
            std::fs::read_to_string(current.join("settings.json")).unwrap(),
            "saved preference"
        );
        assert_eq!(
            std::fs::read_to_string(legacy.join("settings.json")).unwrap(),
            "older preference"
        );
    }
}
