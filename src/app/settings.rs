use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub write_metadata: bool,
}
impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(data) => {
                serde_json::from_slice(&data).map_err(|e| format!("Could not read settings: {e}"))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Could not read settings: {e}")),
        }
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path.parent().ok_or("Invalid settings path")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(
            &temp,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(temp, path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_saved_preference_survive_reload() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("app/settings.json");
        assert!(!Settings::load(&p).unwrap().write_metadata);
        Settings {
            write_metadata: true,
        }
        .save(&p)
        .unwrap();
        assert!(Settings::load(&p).unwrap().write_metadata);
        Settings::default().save(&p).unwrap();
        assert!(!Settings::load(&p).unwrap().write_metadata);
        std::fs::write(&p, "broken").unwrap();
        assert!(Settings::load(&p).is_err());
    }
}
