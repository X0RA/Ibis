//! Library projection and non-destructive per-file preferences.
use crate::media::{ImageEntry, decoder::is_raw};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub use crate::paths::{cache_dir, data_dir};
pub fn key(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(path.as_os_str().as_encoded_bytes()))
}
#[derive(Clone, Default)]
pub struct Edit {
    pub rating: i32,
    pub rejected: bool,
    pub rotation: i32,
}
impl Edit {
    pub fn load(entry: &ImageEntry, embedded: bool) -> Self {
        let value: Value = std::fs::read(data_dir().join(format!("{}.json", key(&entry.path))))
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_default();
        Self::from_values(&entry.metadata, &value, embedded)
    }
    fn from_values(metadata: &Value, local: &Value, embedded: bool) -> Self {
        let embedded_rating = metadata["Rating"].as_f64().unwrap_or(0.) as i32;
        Self {
            rating: if embedded {
                embedded_rating.clamp(0, 5)
            } else {
                local["rating"]
                    .as_i64()
                    .map_or(embedded_rating, |n| n as i32)
                    .clamp(0, 5)
            },
            rejected: if embedded {
                embedded_rating == -1
            } else {
                local["rejected"].as_bool().unwrap_or(embedded_rating == -1)
            },
            rotation: local["rotation"].as_i64().unwrap_or(0).rem_euclid(4) as i32,
        }
    }
    pub fn save_rotation(&self, path: &Path) -> Result<(), String> {
        let dest = data_dir().join(format!("{}.json", key(path)));
        let mut value: Value = std::fs::read(&dest)
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_else(|| json!({}));
        if !value.is_object() {
            value = json!({});
        }
        value["rotation"] = json!(self.rotation);
        std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
        let tmp = dest.with_extension("tmp");
        std::fs::write(&tmp, value.to_string()).map_err(|e| e.to_string())?;
        std::fs::rename(tmp, dest).map_err(|e| e.to_string())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let dir = data_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let dest = dir.join(format!("{}.json", key(path)));
        let tmp = dest.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(
            &tmp,
            json!({"rating":self.rating,"rejected":self.rejected,"rotation":self.rotation})
                .to_string(),
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, dest).map_err(|e| e.to_string())
    }
}
#[derive(Default, Clone)]
pub struct Filter {
    pub kind: i32,
    pub rating: i32,
    pub camera: String,
    pub from: String,
    pub to: String,
    pub pair: bool,
}
impl Filter {
    pub fn matches(&self, e: &ImageEntry, edit: &Edit) -> bool {
        let date = e.date_label();
        (match self.kind {
            1 => !e.video,
            2 => e.video,
            3 => is_raw(&e.format),
            _ => true,
        }) && (match self.rating {
            6 => edit.rejected,
            1..=5 => edit.rating >= self.rating,
            _ => true,
        }) && camera(e)
            .to_lowercase()
            .contains(&self.camera.trim().to_lowercase())
            && (self.from.is_empty() || e.date.is_some() && date[..10] >= self.from[..])
            && (self.to.is_empty() || e.date.is_some() && date[..10] <= self.to[..])
    }
    pub fn validate(&self) -> Result<(), String> {
        for date in [&self.from, &self.to] {
            if !date.is_empty() && chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
                return Err("Dates must use YYYY-MM-DD".into());
            }
        }
        if !self.from.is_empty() && !self.to.is_empty() && self.from > self.to {
            return Err("Start date must precede end date".into());
        }
        Ok(())
    }
}
pub fn camera(e: &ImageEntry) -> String {
    format!(
        "{} {}",
        text(&e.metadata, "Make"),
        text(&e.metadata, "Model")
    )
}
pub fn text(v: &Value, k: &str) -> String {
    match &v[k] {
        Value::String(s) => s.clone(),
        Value::Null => "—".into(),
        v => v.to_string(),
    }
}
pub fn info(e: &ImageEntry) -> String {
    let v = &e.metadata;
    let lens = ["LensModel", "LensID", "Lens"]
        .into_iter()
        .find(|key| !v[*key].is_null())
        .unwrap_or("LensModel");
    format!(
        "{}\n\nCamera  {} {}\nLens  {}\nFocal length  {}\nAperture  {}\nShutter  {} s\nISO  {}\nDimensions  {} × {}\nFile size  {} bytes\nTaken  {}\nGPS latitude  {}\nGPS longitude  {}\n\n{}",
        e.path.file_name().unwrap_or_default().to_string_lossy(),
        text(v, "Make"),
        text(v, "Model"),
        text(v, lens),
        text(v, "FocalLength"),
        text(v, "FNumber"),
        text(v, "ExposureTime"),
        text(v, "ISO"),
        text(v, "ImageWidth"),
        text(v, "ImageHeight"),
        text(v, "FileSize"),
        e.date_label(),
        text(v, "GPSLatitude"),
        text(v, "GPSLongitude"),
        e.path.display()
    )
}
/// Pair only an unambiguous RAW/JPEG with the same case-insensitive stem and folder.
pub fn pairs(entries: &[ImageEntry]) -> HashMap<PathBuf, PathBuf> {
    let mut groups: HashMap<(PathBuf, String), Vec<&ImageEntry>> = HashMap::new();
    for e in entries {
        if is_raw(&e.format) || matches!(e.format.as_str(), "JPEG" | "JPG") {
            groups
                .entry((
                    e.path.parent().unwrap_or(Path::new("")).to_owned(),
                    e.path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase(),
                ))
                .or_default()
                .push(e);
        }
    }
    let mut out = HashMap::new();
    for group in groups.values() {
        if group.len() == 2 {
            let raw = group.iter().find(|e| is_raw(&e.format));
            let jpg = group
                .iter()
                .find(|e| matches!(e.format.as_str(), "JPEG" | "JPG"));
            if let (Some(raw), Some(jpg)) = (raw, jpg) {
                out.insert(jpg.path.clone(), raw.path.clone());
                out.insert(raw.path.clone(), jpg.path.clone());
            }
        }
    }
    out
}

pub fn duration(entry: &ImageEntry) -> String {
    let Some(seconds) = entry.metadata["Duration"]
        .as_f64()
        .filter(|v| v.is_finite() && *v >= 0.)
    else {
        return String::new();
    };
    let seconds = seconds.round() as u64;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}
pub fn source_label(entry: &ImageEntry) -> &'static str {
    match entry.source {
        "EXIF capture date" => "EXIF original",
        "EXIF digitized date" => "EXIF created",
        other => other,
    }
}
pub fn date_label(entry: &ImageEntry) -> String {
    entry
        .date
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%-d %b %Y, %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "Unknown date".into())
}
pub fn detail_rows(entry: &ImageEntry) -> Vec<(String, String)> {
    let value = &entry.metadata;
    let mut rows = Vec::new();
    if entry.video {
        let time = duration(entry);
        rows.push((
            "Duration".into(),
            if time.is_empty() { "—".into() } else { time },
        ));
    }
    rows.push((
        "Dimensions".into(),
        if value["ImageWidth"].is_null() || value["ImageHeight"].is_null() {
            "—".into()
        } else {
            format!(
                "{} × {}",
                text(value, "ImageWidth"),
                text(value, "ImageHeight")
            )
        },
    ));
    let size = value["FileSize"]
        .as_f64()
        .map(|n| {
            if n >= 1_000_000_000. {
                format!("{:.1} GB", n / 1_000_000_000.)
            } else if n >= 1_000_000. {
                format!("{:.1} MB", n / 1_000_000.)
            } else if n >= 1000. {
                format!("{:.0} KB", n / 1000.)
            } else {
                format!("{n:.0} bytes")
            }
        })
        .unwrap_or_else(|| "—".into());
    rows.extend([
        ("Size".into(), size),
        ("Captured".into(), date_label(entry)),
        ("Date source".into(), source_label(entry).into()),
    ]);
    let make = text(value, "Make");
    let model = text(value, "Model");
    let camera = if make == "—" || model.to_lowercase().starts_with(&make.to_lowercase()) {
        model
    } else if model == "—" {
        make
    } else {
        format!("{make} {model}")
    };
    rows.push(("Camera".into(), camera));
    if !entry.video {
        let lens = ["LensModel", "LensID", "Lens"]
            .into_iter()
            .find(|key| !value[*key].is_null())
            .unwrap_or("LensModel");
        rows.extend([
            ("Lens".into(), text(value, lens)),
            ("Focal length".into(), text(value, "FocalLength")),
            (
                "Aperture".into(),
                if value["FNumber"].is_null() {
                    "—".into()
                } else {
                    format!("f/{}", text(value, "FNumber"))
                },
            ),
            (
                "Shutter".into(),
                if value["ExposureTime"].is_null() {
                    "—".into()
                } else {
                    format!("{} s", text(value, "ExposureTime"))
                },
            ),
            ("ISO".into(), text(value, "ISO")),
        ]);
    }
    rows.push((
        "GPS".into(),
        if value["GPSLatitude"].is_null() && value["GPSLongitude"].is_null() {
            "—".into()
        } else {
            format!(
                "{}\n{}",
                text(value, "GPSLatitude"),
                text(value, "GPSLongitude")
            )
        },
    ));
    rows.push(("Path".into(), entry.path.to_string_lossy().into_owned()));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    fn e(name: &str, format: &str) -> ImageEntry {
        ImageEntry {
            path: name.into(),
            format: format.into(),
            orientation: 1,
            video: false,
            date: None,
            source: "Unknown",
            metadata: Value::Null,
        }
    }
    #[test]
    fn storage_modes_use_the_selected_source_and_share_only_rotation() {
        let file = json!({"Rating":-1});
        let local = json!({"rating":4,"rejected":false,"rotation":2});
        let embedded = Edit::from_values(&file, &local, true);
        assert!(embedded.rejected);
        assert_eq!(embedded.rating, 0);
        assert_eq!(embedded.rotation, 2);
        let json = Edit::from_values(&file, &local, false);
        assert!(!json.rejected);
        assert_eq!(json.rating, 4);
        assert!(Edit::from_values(&file, &Value::Null, false).rejected);
    }
    #[test]
    fn pairs_require_same_folder_and_one_of_each() {
        let p = pairs(&[
            e("/a/shot.RAF", "RAF"),
            e("/a/SHOT.jpg", "JPEG"),
            e("/b/shot.jpg", "JPEG"),
        ]);
        assert_eq!(p.len(), 2);
        assert_eq!(p[Path::new("/a/shot.RAF")], Path::new("/a/SHOT.jpg"));
        assert!(
            pairs(&[
                e("/a/s.RAF", "RAF"),
                e("/a/s.NEF", "NEF"),
                e("/a/s.JPG", "JPEG")
            ])
            .is_empty()
        );
    }
    #[test]
    fn validates_dates_and_filters_unknown_dates() {
        let f = Filter {
            from: "2024-01-01".into(),
            ..Default::default()
        };
        assert!(f.validate().is_ok());
        assert!(!f.matches(&e("s.jpg", "JPEG"), &Edit::default()));
        assert!(
            Filter {
                from: "bad".into(),
                ..f
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn type_camera_rating_and_rejection_filters_compose() {
        let mut entry = e("/a/shot.NEF", "NEF");
        entry.metadata = json!({"Model":"Nikon D850"});
        let mut f = Filter {
            kind: 3,
            camera: "d850".into(),
            rating: 4,
            ..Default::default()
        };
        let edit = Edit {
            rating: 5,
            rejected: false,
            rotation: 0,
        };
        assert!(f.matches(&entry, &edit));
        f.kind = 2;
        assert!(!f.matches(&entry, &edit));
        f.kind = 1;
        assert!(f.matches(&entry, &edit));
        f.rating = 6;
        assert!(!f.matches(&entry, &edit));
        assert!(f.matches(
            &entry,
            &Edit {
                rejected: true,
                ..edit
            }
        ));
    }
    #[test]
    fn camera_filter_matches_make_and_model() {
        let mut entry = e("/a/test.RAF", "RAF");
        entry.metadata = json!({"Make":"FUJIFILM", "Model":"X-S10"});
        for camera in ["Fujifilm", "x-s10", " fujifilm x-s10 "] {
            assert!(
                Filter {
                    camera: camera.into(),
                    ..Default::default()
                }
                .matches(&entry, &Edit::default())
            );
        }
        assert!(
            !Filter {
                camera: "DJI".into(),
                ..Default::default()
            }
            .matches(&entry, &Edit::default())
        );
    }
    #[test]
    fn inspector_formats_metadata_and_missing_values() {
        let mut entry = e("/a/test.jpg", "JPEG");
        entry.metadata = json!({"Make":"Nikon", "Model":"Nikon D850", "ImageWidth":8256,
            "ImageHeight":5504, "FileSize":12400000, "FNumber":2.8, "ExposureTime":"1/250"});
        let rows: std::collections::HashMap<_, _> = detail_rows(&entry).into_iter().collect();
        assert_eq!(rows["Camera"], "Nikon D850");
        assert_eq!(rows["Dimensions"], "8256 × 5504");
        assert_eq!(rows["Size"], "12.4 MB");
        assert_eq!(rows["Aperture"], "f/2.8");
        assert_eq!(rows["Shutter"], "1/250 s");
        assert_eq!(rows["Lens"], "—");
        assert_eq!(rows["GPS"], "—");
        assert_eq!(rows["Path"], "/a/test.jpg");
    }
    #[test]
    fn video_duration_handles_hours_rounding_and_missing_metadata() {
        let mut entry = e("/a/clip.mp4", "MP4");
        assert_eq!(duration(&entry), "");
        for (seconds, expected) in [(0., "0:00"), (59.8, "1:00"), (3661., "1:01:01"), (-1., "")] {
            entry.metadata = json!({"Duration":seconds});
            assert_eq!(duration(&entry), expected);
        }
    }
}
