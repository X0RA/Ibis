use std::{path::Path, process::Command};

/// Write only XMP Rating. ExifTool performs its normal temporary-file replacement;
/// -P preserves capture-date fallbacks based on the file modification time.
pub fn write_rating(path: &Path, rating: i32) -> Result<(), String> {
    if !(-1..=5).contains(&rating) {
        return Err("Invalid rating".into());
    }
    let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    if std::fs::metadata(&path)
        .map_err(|e| e.to_string())?
        .permissions()
        .readonly()
    {
        return Err("The file is read-only".into());
    }
    let output = Command::new("exiftool")
        .args(["-overwrite_original", "-P", "-charset", "filename=UTF8"])
        .arg(format!("-XMP-xmp:Rating={rating}"))
        .arg("--")
        .arg(&path)
        .output()
        .map_err(|e| format!("Could not run ExifTool: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Metadata was not saved: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let verify = Command::new("exiftool")
        .args([
            "-json",
            "-n",
            "-charset",
            "filename=UTF8",
            "-XMP-xmp:Rating",
            "--",
        ])
        .arg(&path)
        .output()
        .map_err(|e| e.to_string())?;
    let records: serde_json::Value =
        serde_json::from_slice(&verify.stdout).map_err(|e| e.to_string())?;
    if !verify.status.success() || records[0]["Rating"].as_f64() != Some(rating as f64) {
        return Err("Could not verify the embedded rating after writing".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writes_stars_rejection_and_unrated_without_changing_pixels() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("photo.jpg");
        std::fs::copy(crate::test_image("common/sample.jpg"), &path).unwrap();
        let before = image::open(&path).unwrap().to_rgb8();
        for rating in [4, -1, 0, 5] {
            write_rating(&path, rating).unwrap();
            assert_eq!(image::open(&path).unwrap().to_rgb8(), before);
            let report = crate::media::scan(d.path()).unwrap();
            assert_eq!(
                report.images[0].metadata["Rating"].as_i64(),
                Some(rating as i64)
            );
        }
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
        let unsupported = d.path().join("bad.txt");
        std::fs::write(&unsupported, "unchanged").unwrap();
        assert!(write_rating(&unsupported, 3).is_err());
        assert_eq!(std::fs::read_to_string(unsupported).unwrap(), "unchanged");
    }
}
