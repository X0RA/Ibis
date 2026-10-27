use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use serde_json::Value;
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct ImageEntry {
    pub path: PathBuf,
    pub format: String,
    pub orientation: u8,
    pub video: bool,
    pub date: Option<DateTime<Utc>>,
    pub metadata: Value,
    pub source: &'static str,
}

impl ImageEntry {
    pub fn date_label(&self) -> String {
        self.date
            .map(|date| {
                date.with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M:%S%.3f")
                    .to_string()
            })
            .unwrap_or_else(|| "Unknown date".into())
    }
}

#[derive(Debug, Default)]
pub struct ScanReport {
    pub images: Vec<ImageEntry>,
    pub warnings: Vec<String>,
}

/// Discover by content using ExifTool, without decoding image pixels.
/// Directory symlinks are not followed, preventing cycles and escapes.
pub fn scan(directory: &Path) -> io::Result<ScanReport> {
    scan_depth(directory, true)
}

pub fn scan_depth(directory: &Path, recursive: bool) -> io::Result<ScanReport> {
    scan_depth_with_progress(directory, recursive, |_, _| {})
}

/// Report checked files against a fixed total, after discovering the input set.
/// `None` denotes directory discovery, where a percentage is not yet meaningful.
pub fn scan_depth_with_progress(
    directory: &Path,
    recursive: bool,
    mut progress: impl FnMut(usize, Option<usize>),
) -> io::Result<ScanReport> {
    progress(0, None);
    let directory = fs::canonicalize(directory)?;
    if !directory.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Please choose a directory",
        ));
    }
    let mut report = ScanReport::default();
    let mut files = Vec::new();
    // Never re-import our own generated thumbnails or files in the user's trash.
    let excluded = [
        crate::paths::cache_dir(),
        crate::paths::data_dir(),
        crate::paths::config_dir(),
        crate::paths::data_base().join("Trash"),
    ];
    for entry in WalkDir::new(&directory)
        .follow_links(false)
        .max_depth(if recursive { usize::MAX } else { 1 })
        .into_iter()
        .filter_entry(|e| !excluded.iter().any(|p| e.path() == p))
    {
        match entry {
            Ok(entry) if entry.file_type().is_file() => {
                files.push(entry.into_path());
            }
            Ok(_) => {}
            Err(error) => report.warnings.push(error.to_string()),
        }
    }
    let total = files.len();
    progress(0, Some(total));
    let mut completed = 0;
    for batch in files.chunks(64) {
        inspect_batch(batch, &mut report)?;
        completed += batch.len();
        progress(completed, Some(total));
    }
    report.images.sort_by(|a, b| match (a.date, b.date) {
        (Some(a_date), Some(b_date)) => a_date.cmp(&b_date).then_with(|| a.path.cmp(&b.path)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.path.cmp(&b.path),
    });
    Ok(report)
}

fn inspect_batch(paths: &[PathBuf], report: &mut ScanReport) -> io::Result<()> {
    let output = Command::new("exiftool")
        .args([
            "-json",
            "-G1",
            "-charset",
            "filename=UTF8",
            "-Make",
            "-Model",
            "-LensModel",
            "-LensID",
            "-Lens",
            "-FocalLength",
            "-FNumber",
            "-ExposureTime",
            "-ISO",
            "-ImageWidth",
            "-ImageHeight",
            "-FileSize#",
            "-Duration#",
            "-GPSLatitude",
            "-GPSLongitude",
            "-Rating",
            "-XMP-xmp:Rating",
            "-MIMEType",
            "-FileType",
            "-Orientation#",
            "-SubSecDateTimeOriginal",
            "-DateTimeOriginal",
            "-SubSecCreateDate",
            "-CreateDate",
            "-OffsetTimeOriginal",
            "-OffsetTimeDigitized",
        ])
        .args(paths)
        .output()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("Could not run ExifTool. Install exiftool to scan images: {error}"),
            )
        })?;
    let records: Vec<Value> = serde_json::from_slice(&output.stdout)
        .map_err(|error| io::Error::other(format!("Invalid ExifTool response: {error}")))?;
    for record in records {
        let mut record = record;
        if let Some(tags) = record.as_object_mut() {
            for (key, value) in tags.clone() {
                if let Some((_, name)) = key.split_once(':') {
                    tags.entry(name.to_string()).or_insert(value);
                }
            }
            if let Some(rating) = tags.get("XMP-xmp:Rating").cloned() {
                tags.insert("Rating".into(), rating);
            }
        }
        let Some(path) = record["SourceFile"].as_str().map(PathBuf::from) else {
            continue;
        };
        // ExifTool recognizes image containers and camera RAW formats by their content,
        // including files with missing, uppercase, or incorrect extensions.
        let is_image = record["MIMEType"]
            .as_str()
            .is_some_and(|mime| mime.starts_with("image/"));
        let is_video = record["MIMEType"]
            .as_str()
            .is_some_and(|mime| mime.starts_with("video/"));
        if !is_image && !is_video {
            if let Some(error) = record["Error"].as_str()
                && error != "Unknown file type"
                && error != "File is empty"
            {
                report.warnings.push(format!("{}: {error}", path.display()));
            }
            continue;
        }
        if let Some(warning) = record["Warning"].as_str().or(record["Error"].as_str()) {
            report
                .warnings
                .push(format!("{}: {warning}", path.display()));
        }
        let capture = capture_date(&record);
        let (date, source) = match capture {
            Some((date, source)) => (Some(date), source),
            None => match fs::metadata(&path) {
                Ok(metadata) => match metadata.modified().or_else(|_| metadata.created()) {
                    Ok(date) => (Some(DateTime::<Utc>::from(date)), "File timestamp"),
                    Err(_) => (None, "Unknown"),
                },
                Err(error) => {
                    report.warnings.push(format!("{}: {error}", path.display()));
                    (None, "Unknown")
                }
            },
        };
        report.images.push(ImageEntry {
            metadata: record.clone(),
            video: is_video,
            orientation: record["Orientation"].as_u64().unwrap_or(1) as u8,
            path,
            format: record["FileType"].as_str().unwrap_or("IMG").to_owned(),
            date,
            source,
        });
    }
    if !output.status.success() && report.warnings.is_empty() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !message.is_empty() {
            report.warnings.push(message);
        }
    }
    Ok(())
}

fn capture_date(record: &Value) -> Option<(DateTime<Utc>, &'static str)> {
    for (tag, offset, source) in [
        (
            "SubSecDateTimeOriginal",
            "OffsetTimeOriginal",
            "EXIF capture date",
        ),
        (
            "DateTimeOriginal",
            "OffsetTimeOriginal",
            "EXIF capture date",
        ),
        (
            "SubSecCreateDate",
            "OffsetTimeDigitized",
            "EXIF digitized date",
        ),
        ("CreateDate", "OffsetTimeDigitized", "EXIF digitized date"),
    ] {
        if let Some(text) = record[tag].as_str()
            && let Some(date) = parse_date(text, record[offset].as_str())
        {
            return Some((date, source));
        }
    }
    None
}

fn parse_date(text: &str, offset: Option<&str>) -> Option<DateTime<Utc>> {
    if let Ok(date) = DateTime::parse_from_str(text, "%Y:%m:%d %H:%M:%S%.f%:z") {
        return Some(date.with_timezone(&Utc));
    }
    if let Some(offset) = offset
        && let Ok(date) =
            DateTime::parse_from_str(&format!("{text}{offset}"), "%Y:%m:%d %H:%M:%S%.f%:z")
    {
        return Some(date.with_timezone(&Utc));
    }
    let naive = NaiveDateTime::parse_from_str(text, "%Y:%m:%d %H:%M:%S%.f").ok()?;
    // Camera dates often omit a time zone. Interpret these in the desktop's local
    // zone; use the first occurrence for ambiguous DST times and UTC for gaps.
    Some(
        Local
            .from_local_datetime(&naive)
            .earliest()
            .map(|date| date.with_timezone(&Utc))
            .unwrap_or_else(|| naive.and_utc()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn progress_counts_checked_files_and_finishes_at_the_exact_total() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..130 {
            fs::write(dir.path().join(format!("{i}.txt")), "not an image").unwrap();
        }
        let mut updates = Vec::new();
        let report =
            scan_depth_with_progress(dir.path(), true, |done, total| updates.push((done, total)))
                .unwrap();
        assert!(report.images.is_empty());
        assert_eq!(
            updates,
            [
                (0, None),
                (0, Some(130)),
                (64, Some(130)),
                (128, Some(130)),
                (130, Some(130))
            ]
        );
        let empty = tempfile::tempdir().unwrap();
        updates.clear();
        scan_depth_with_progress(empty.path(), true, |done, total| {
            updates.push((done, total))
        })
        .unwrap();
        assert_eq!(updates, [(0, None), (0, Some(0))]);
    }

    #[test]
    fn capture_priority_offsets_subseconds_and_invalid_dates() {
        let record = json!({"SubSecDateTimeOriginal":"2024:10:26 19:53:08.123+11:00",
            "CreateDate":"2025:01:01 00:00:00"});
        let (date, source) = capture_date(&record).unwrap();
        assert_eq!(date.to_rfc3339(), "2024-10-26T08:53:08.123+00:00");
        assert_eq!(source, "EXIF capture date");
        assert_eq!(
            parse_date("2024:10:26 19:53:08", Some("+11:00"))
                .unwrap()
                .timestamp(),
            date.timestamp()
        );
        assert!(parse_date("0000:00:00 00:00:00", None).is_none());
        assert_eq!(
            capture_date(
                &json!({"DateTimeOriginal":"bad", "CreateDate":"2024:01:01 00:00:00+00:00"})
            )
            .unwrap()
            .1,
            "EXIF digitized date"
        );
    }

    #[test]
    fn videos_are_discovered_by_content_even_with_photo_extensions() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("clip.JPG");
        fs::copy(crate::test_image("video/sample.mp4"), &path).unwrap();
        let report = scan(root.path()).unwrap();
        assert_eq!(report.images.len(), 1);
        assert!(report.images[0].video);
        assert_eq!(report.images[0].format, "MP4");
        assert_eq!(report.images[0].path, path);
    }

    #[test]
    fn scans_nested_content_and_sorts_timestamp_fallbacks() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("nested")).unwrap();
        let older = root.path().join("nested/older.data");
        let newer = root.path().join("NEWER.GIF");
        fs::copy(crate::test_image("common/sample.gif"), &older).unwrap();
        fs::copy(crate::test_image("common/sample.gif"), &newer).unwrap();
        for (path, seconds) in [(&older, 1000), (&newer, 2000)] {
            fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_times(
                    fs::FileTimes::new().set_modified(
                        std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds),
                    ),
                )
                .unwrap();
        }
        fs::write(root.path().join("notes.txt"), "not an image").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.path(), root.path().join("nested/loop")).unwrap();
        let report = scan(root.path()).unwrap();
        assert_eq!(report.images.len(), 2);
        assert_eq!(report.images[0].path, older);
        assert_eq!(report.images[1].path, newer);
        assert_eq!(report.images[0].source, "File timestamp");
        assert_eq!(report.images[0].format, "GIF");
        assert!(!report.images[0].video);
        assert_eq!(report.images[0].date.unwrap().timestamp(), 1000);
        assert!(scan(&root.path().join("notes.txt")).is_err());
        assert!(scan(&root.path().join("missing")).is_err());
        let empty = tempfile::tempdir().unwrap();
        assert!(scan(empty.path()).unwrap().images.is_empty());
    }
    #[test]
    fn file_open_navigation_can_scan_only_the_containing_folder() {
        let d = tempfile::tempdir().unwrap();
        let nested = d.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::copy(
            crate::test_image("common/sample.jpg"),
            d.path().join("one.jpg"),
        )
        .unwrap();
        fs::copy(
            crate::test_image("common/sample.jpg"),
            nested.join("two.jpg"),
        )
        .unwrap();
        assert_eq!(scan_depth(d.path(), false).unwrap().images.len(), 1);
        assert_eq!(scan_depth(d.path(), true).unwrap().images.len(), 2);
    }
}
