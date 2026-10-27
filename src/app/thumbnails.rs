use super::catalog;
use crate::media::{
    ImageEntry,
    decoder::{self, Cancellation, DecodeRequest},
};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64, mpsc},
};

pub struct Worker {
    pub requests: mpsc::Sender<(u64, Vec<ImageEntry>)>,
    pub results: mpsc::Receiver<(u64, PathBuf, Result<PathBuf, String>)>,
}
impl Worker {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<(u64, Vec<ImageEntry>)>();
        let (out, results) = mpsc::channel();
        std::thread::spawn(move || {
            prune_cache();
            let mut count = 0;
            let cancel = Cancellation {
                generation: Arc::new(AtomicU64::new(0)),
                id: 0,
            };
            let mut pending = std::collections::VecDeque::new();
            let mut generation = 0;
            loop {
                if pending.is_empty() {
                    let Ok((id, entries)) = rx.recv() else { break };
                    generation = id;
                    pending = entries.into();
                }
                while let Ok((id, entries)) = rx.try_recv() {
                    generation = id;
                    pending = entries.into();
                }
                let Some(entry) = pending.pop_front() else {
                    continue;
                };
                let result = thumbnail(&entry, &cancel);
                count += 1;
                if count % 128 == 0 {
                    prune_cache();
                }
                if out.send((generation, entry.path, result)).is_err() {
                    break;
                }
            }
        });
        Self {
            requests: tx,
            results,
        }
    }
}
fn thumbnail(e: &ImageEntry, cancel: &Cancellation) -> Result<PathBuf, String> {
    let meta = std::fs::metadata(&e.path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    hash.update(e.path.as_os_str().as_encoded_bytes());
    hash.update(format!(
        "v1:{}:{:?}:{}",
        meta.len(),
        meta.modified(),
        e.orientation
    ));
    let dir = catalog::cache_dir().join("thumbnails");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{:x}.png", hash.finalize()));
    if path.exists() {
        return Ok(path);
    }
    let pixels = if e.video {
        let output = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&e.path)
            .args([
                "-frames:v",
                "1",
                "-vf",
                "scale=256:256:force_original_aspect_ratio=decrease",
                "-f",
                "image2pipe",
                "-vcodec",
                "png",
                "-",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err("No video thumbnail".into());
        }
        image::load_from_memory(&output.stdout)
            .map_err(|e| e.to_string())?
            .to_rgba8()
    } else {
        decoder::decode(
            &DecodeRequest {
                path: &e.path,
                format: &e.format,
                orientation: e.orientation,
                full: false,
                edge: 256,
            },
            cancel,
        )?
        .pixels
    };
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    pixels
        .save_with_format(&tmp, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    std::fs::rename(tmp, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Bound disk usage to 512 MiB, removing the oldest generated thumbnails first.
fn prune_cache() {
    let Ok(files) = std::fs::read_dir(catalog::cache_dir().join("thumbnails")) else {
        return;
    };
    let mut files = files
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|e| e == "png"))
        .filter_map(|e| {
            e.metadata()
                .ok()
                .map(|m| (e.path(), m.len(), m.modified().ok()))
        })
        .collect::<Vec<_>>();
    let mut size: u64 = files.iter().map(|(_, n, _)| n).sum();
    files.sort_by_key(|(_, _, time)| *time);
    for (path, n, _) in files {
        if size <= 512 * 1024 * 1024 {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            size = size.saturating_sub(n)
        }
    }
}
