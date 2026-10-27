use super::AppWindow;
use crate::media::{
    ImageEntry,
    decoder::{self, Cancellation, DecodeRequest, Decoded},
};
use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::SystemTime,
};

const CACHE_BYTES: usize = 256 * 1024 * 1024;
const WINDOW_RADIUS: usize = 3;

/// Nearest neighbors first, favoring the direction of travel at each distance.
pub(super) fn neighbors(entries: &[ImageEntry], index: usize, backwards: bool) -> Vec<ImageEntry> {
    let mut result = Vec::with_capacity(WINDOW_RADIUS * 2);
    for distance in 1..=WINDOW_RADIUS {
        let before = index.checked_sub(distance);
        let after = index.checked_add(distance).filter(|&i| i < entries.len());
        let pair = if backwards {
            [before, after]
        } else {
            [after, before]
        };
        for i in pair.into_iter().flatten() {
            if let Some(entry) = entries.get(i)
                && !entry.video
            {
                result.push(entry.clone());
            }
        }
    }
    result
}
#[derive(Clone)]
struct Job {
    entry: ImageEntry,
    neighbors: Vec<ImageEntry>,
    full: bool,
    edge: u32,
    id: u64,
}
#[derive(Clone, PartialEq, Eq)]
struct Key {
    path: PathBuf,
    modified: Option<SystemTime>,
    size: u64,
    full: bool,
    edge: u32,
}
impl Key {
    fn new(entry: &ImageEntry, full: bool, edge: u32) -> Self {
        let meta = std::fs::metadata(&entry.path).ok();
        Self {
            path: entry.path.clone(),
            modified: meta.as_ref().and_then(|m| m.modified().ok()),
            size: meta.map_or(0, |m| m.len()),
            full,
            edge: if full { 0 } else { edge },
        }
    }
}
struct Cache {
    items: VecDeque<(Key, Arc<Frame>)>,
    bytes: usize,
    window: Vec<Key>,
    budget: usize,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            items: VecDeque::new(),
            bytes: 0,
            window: Vec::new(),
            budget: CACHE_BYTES,
        }
    }
}
impl Cache {
    fn set_window(&mut self, keys: Vec<Key>) {
        self.window = keys;
        self.items.retain(|(key, _)| self.window.contains(key));
        self.bytes = self.items.iter().map(|(_, frame)| frame.bytes()).sum();
        self.prioritize();
    }
    fn prioritize(&mut self) {
        if !self.window.is_empty() {
            self.items.make_contiguous().sort_by_key(|(key, _)| {
                self.window
                    .iter()
                    .position(|k| k == key)
                    .unwrap_or(usize::MAX)
            });
        }
    }
    fn contains(&self, key: &Key) -> bool {
        self.items.iter().any(|(k, _)| k == key)
    }

    fn get(&mut self, key: &Key) -> Option<Arc<Frame>> {
        let i = self.items.iter().position(|(k, _)| k == key)?;
        let item = self.items.remove(i)?;
        let value = item.1.clone();
        self.items.push_front(item);
        self.prioritize();
        Some(value)
    }
    fn insert(&mut self, key: Key, value: Arc<Frame>) {
        let size = value.bytes();
        // Quality is part of the key. Reject late results from an old window/mode.
        if size > self.budget || (!self.window.is_empty() && !self.window.contains(&key)) {
            return;
        }
        if let Some(i) = self.items.iter().position(|(k, _)| k == &key) {
            let (_, old) = self.items.remove(i).unwrap();
            self.bytes -= old.bytes();
        }
        self.bytes += size;
        self.items.push_front((key, value));
        self.prioritize();
        // Evict the farthest frame first, never a nearby frame for a distant preload.
        while self.bytes > self.budget {
            if let Some((_, old)) = self.items.pop_back() {
                self.bytes -= old.bytes();
            }
        }
    }
}
/// A decoded, UI-ready buffer. Cloning it shares pixels; cached navigation never
/// copies a full image or waits for a decoder worker.
struct Frame {
    buffer: SharedPixelBuffer<Rgba8Pixel>,
    original: (u32, u32),
    preview: bool,
    note: String,
}
impl Frame {
    fn from_decoded(decoded: Decoded) -> Self {
        Self {
            buffer: SharedPixelBuffer::clone_from_slice(
                decoded.pixels.as_raw(),
                decoded.pixels.width(),
                decoded.pixels.height(),
            ),
            original: decoded.original,
            preview: decoded.preview,
            note: decoded.note,
        }
    }
    fn bytes(&self) -> usize {
        self.buffer.as_bytes().len()
    }
}

#[derive(Default)]
struct Mailbox {
    pending: Mutex<Option<Job>>,
    wake: Condvar,
}
impl Mailbox {
    fn take(&self, stopped: &AtomicBool) -> Option<Job> {
        let mut pending = self.pending.lock().unwrap();
        while pending.is_none() && !stopped.load(Ordering::Relaxed) {
            pending = self.wake.wait(pending).unwrap();
        }
        if stopped.load(Ordering::Relaxed) {
            None
        } else {
            pending.take()
        }
    }
    fn put(&self, job: Job, generation: &AtomicU64) {
        let mut pending = self.pending.lock().unwrap();
        if generation.load(Ordering::Relaxed) == job.id {
            *pending = Some(job);
            self.wake.notify_one();
        }
    }
    fn clear(&self) {
        *self.pending.lock().unwrap() = None;
        self.wake.notify_one();
    }
}
struct Shared {
    foreground: Mailbox,
    prefetch: Mailbox,
    cache: Mutex<Cache>,
    generation: Arc<AtomicU64>,
    stopped: AtomicBool,
}
impl Default for Shared {
    fn default() -> Self {
        Self {
            foreground: Mailbox::default(),
            prefetch: Mailbox::default(),
            cache: Mutex::new(Cache::default()),
            generation: Arc::new(AtomicU64::new(0)),
            stopped: AtomicBool::new(false),
        }
    }
}
pub struct Viewer {
    shared: Arc<Shared>,
}
impl Viewer {
    pub fn new(window: &AppWindow, cache_bytes: usize) -> Self {
        let shared = Arc::new(Shared::default());
        shared.cache.lock().unwrap().budget = cache_bytes;
        let worker = shared.clone();
        let weak = window.as_weak();
        std::thread::spawn(move || {
            while let Some(job) = worker.foreground.take(&worker.stopped) {
                let cancel = Cancellation {
                    generation: worker.generation.clone(),
                    id: job.id,
                };
                let mut preview_shown = false;
                let mut preview_ready = |decoded| {
                    if cancel.cancelled() {
                        return;
                    }
                    let frame = Frame::from_decoded(decoded);
                    let generation = worker.generation.clone();
                    let id = job.id;
                    preview_shown = true;
                    let _ = weak.upgrade_in_event_loop(move |window| {
                        if generation.load(Ordering::Relaxed) != id || !window.get_viewer_open() {
                            return;
                        }
                        show_frame(&window, &frame, false);
                        window.set_photo_loading(true);
                        window.set_photo_status("Camera preview · Developing full quality…".into());
                    });
                };
                let result = load_progressive(
                    &job.entry,
                    job.full,
                    job.edge,
                    &cancel,
                    &worker.cache,
                    Some(&mut preview_ready),
                );
                if cancel.cancelled() {
                    continue;
                }
                let generation = worker.generation.clone();
                let id = job.id;
                let full = job.full;
                let _ = weak.upgrade_in_event_loop(move |window| {
                    if generation.load(Ordering::Relaxed) != id || !window.get_viewer_open() {
                        return;
                    }
                    window.set_photo_loading(false);
                    match result {
                        Ok(frame) => show_frame(&window, &frame, full),
                        Err(error) if preview_shown => {
                            window.set_photo_status(
                                format!("Camera preview · Full quality failed: {error}").into(),
                            );
                        }
                        Err(error) => {
                            window.set_photo_error(format!("Could not open photo: {error}").into());
                            window.set_photo_status("Use the arrows to continue browsing".into());
                        }
                    }
                });
                worker.prefetch.put(job, &worker.generation);
            }
        });
        // A separate, single speculative worker cannot block foreground requests.
        // Both queues retain only their newest job; cancellation invalidates old work.
        let worker = shared.clone();
        std::thread::spawn(move || {
            while let Some(job) = worker.prefetch.take(&worker.stopped) {
                let cancel = Cancellation {
                    generation: worker.generation.clone(),
                    id: job.id,
                };
                for neighbor in &job.neighbors {
                    if cancel.cancelled() {
                        break;
                    }
                    let key = Key::new(neighbor, job.full, job.edge);
                    if worker.cache.lock().unwrap().contains(&key) {
                        continue;
                    }
                    if load(neighbor, job.full, job.edge, &cancel, &worker.cache).is_ok()
                        && !worker.cache.lock().unwrap().contains(&key)
                    {
                        break; // Nearer frames fill the byte budget.
                    }
                }
            }
        });
        Self { shared }
    }
    pub fn open(
        &self,
        window: &AppWindow,
        entry: &ImageEntry,
        neighbors: Vec<ImageEntry>,
        full: bool,
    ) {
        let id = self.shared.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.shared.foreground.clear();
        self.shared.prefetch.clear();
        let size = window.window().size();
        let edge = size.width.max(size.height).clamp(1024, 4096);
        let job = Job {
            entry: entry.clone(),
            neighbors,
            full,
            edge,
            id,
        };
        let frame = prepare_cache(&self.shared.cache, &job);
        window.set_photo_error("".into());
        if let Some(frame) = frame {
            show_frame(window, &frame, full);
            self.shared.prefetch.put(job, &self.shared.generation);
        } else {
            window.set_photo_loading(true);
            window.set_photo_status(
                if full {
                    "Loading full quality…"
                } else {
                    "Loading photo…"
                }
                .into(),
            );
            self.shared.foreground.put(job, &self.shared.generation);
        }
    }
    pub fn cancel(&self) {
        self.shared.generation.fetch_add(1, Ordering::Relaxed);
        self.shared.foreground.clear();
        self.shared.prefetch.clear();
    }
}
impl Drop for Viewer {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Relaxed);
        self.cancel();
    }
}
fn prepare_cache(cache: &Mutex<Cache>, job: &Job) -> Option<Arc<Frame>> {
    let keys: Vec<_> = std::iter::once(&job.entry)
        .chain(job.neighbors.iter())
        .map(|entry| Key::new(entry, job.full, job.edge))
        .collect();
    let current = keys[0].clone();
    let mut cache = cache.lock().unwrap();
    cache.set_window(keys);
    cache.get(&current)
}
fn show_frame(window: &AppWindow, frame: &Frame, full: bool) {
    window.set_photo_loading(false);
    window.set_photo_width(frame.buffer.width() as f32);
    window.set_photo_height(frame.buffer.height() as f32);
    window.set_photo_original_width(frame.original.0 as f32);
    window.set_photo(slint::Image::from_rgba8(frame.buffer.clone()));
    window.set_photo_full(full);
    let kind = if frame.preview {
        "Camera preview"
    } else if full {
        "Full quality"
    } else {
        "Fit preview"
    };
    let mut status = format!("{kind} · {} × {}", frame.original.0, frame.original.1);
    if !frame.note.is_empty() {
        status.push_str(&format!(" · {}", frame.note));
    }
    window.set_photo_status(status.into());
    window.set_photo_error("".into());
}
fn load(
    entry: &ImageEntry,
    full: bool,
    edge: u32,
    cancel: &Cancellation,
    cache: &Mutex<Cache>,
) -> Result<Arc<Frame>, String> {
    load_progressive(entry, full, edge, cancel, cache, None)
}
fn load_progressive(
    entry: &ImageEntry,
    full: bool,
    edge: u32,
    cancel: &Cancellation,
    cache: &Mutex<Cache>,
    preview_ready: Option<&mut dyn FnMut(Decoded)>,
) -> Result<Arc<Frame>, String> {
    cancel.check()?;
    let key = Key::new(entry, full, edge);
    if let Some(result) = cache.lock().unwrap().get(&key) {
        return Ok(result);
    }
    // Never hold the cache lock during decoding or pixel-buffer allocation.
    let request = DecodeRequest {
        path: &entry.path,
        format: &entry.format,
        orientation: entry.orientation,
        full,
        edge,
    };
    let decoded = if preview_ready.is_some() {
        decoder::decode_progressive(&request, cancel, preview_ready)
    } else {
        decoder::decode(&request, cancel)
    }?;
    cancel.check()?;
    let frame = Arc::new(Frame::from_decoded(decoded));
    let mut cache = cache.lock().unwrap();
    cancel.check()?;
    cache.insert(key, frame.clone());
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(index: usize) -> ImageEntry {
        ImageEntry {
            metadata: Default::default(),
            path: PathBuf::from(format!("photo-{index}")),
            format: "PNG".into(),
            video: false,
            orientation: 1,
            date: None,
            source: "Unknown",
        }
    }
    fn frame() -> Arc<Frame> {
        Arc::new(Frame {
            buffer: SharedPixelBuffer::new(2, 2),
            original: (2, 2),
            preview: false,
            note: String::new(),
        })
    }
    fn keys(indices: &[usize]) -> Vec<Key> {
        indices
            .iter()
            .map(|&i| Key::new(&entry(i), false, 100))
            .collect()
    }

    #[test]
    fn preload_window_prioritizes_direction_and_handles_folder_edges() {
        let entries: Vec<_> = (0..10).map(entry).collect();
        let paths = |items: Vec<ImageEntry>| items.into_iter().map(|e| e.path).collect::<Vec<_>>();
        let expected =
            |indices: &[usize]| indices.iter().map(|&i| entry(i).path).collect::<Vec<_>>();
        assert_eq!(
            paths(neighbors(&entries, 4, false)),
            expected(&[5, 3, 6, 2, 7, 1])
        );
        assert_eq!(
            paths(neighbors(&entries, 4, true)),
            expected(&[3, 5, 2, 6, 1, 7])
        );
        assert_eq!(paths(neighbors(&entries, 0, false)), expected(&[1, 2, 3]));
        assert_eq!(paths(neighbors(&entries, 9, true)), expected(&[8, 7, 6]));
        assert!(neighbors(&entries[..1], 0, false).is_empty());
        assert!(neighbors(&[], 0, false).is_empty());
    }

    #[test]
    fn photo_preloading_skips_videos_in_a_mixed_library() {
        let mut entries: Vec<_> = (0..7).map(entry).collect();
        entries[2].video = true;
        entries[4].video = true;
        let nearby = neighbors(&entries, 3, false);
        assert!(nearby.iter().all(|entry| !entry.video));
        assert_eq!(
            nearby
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>(),
            vec![entry(5).path, entry(1).path, entry(6).path, entry(0).path]
        );
    }

    #[test]
    fn moving_window_reuses_overlapping_frames_and_releases_old_ones() {
        let mut cache = Cache::default();
        let first = keys(&[3, 4, 2, 5, 1, 6, 0]);
        cache.set_window(first.clone());
        let old = frame();
        let shared = frame();
        cache.insert(first[6].clone(), old.clone());
        cache.insert(first[1].clone(), shared.clone());
        let next = keys(&[4, 5, 3, 6, 2, 7, 1]);
        cache.set_window(next.clone());
        assert_eq!(Arc::strong_count(&old), 1);
        assert!(Arc::ptr_eq(&cache.get(&next[0]).unwrap(), &shared));
        assert_eq!(cache.bytes, 16);
    }

    #[test]
    fn preload_budget_protects_current_and_nearest_frames() {
        let mut cache = Cache {
            budget: 48,
            ..Cache::default()
        };
        let window = keys(&[3, 4, 2, 5, 1, 6, 0]);
        cache.set_window(window.clone());
        for key in &window {
            cache.insert(key.clone(), frame());
        }
        assert_eq!(cache.bytes, 48);
        for key in &window[..3] {
            assert!(cache.contains(key));
        }
        for key in &window[3..] {
            assert!(!cache.contains(key));
        }
        // Replacing an entry must not double-count it, and a late result
        // for a different quality mode must not evict current previews.
        cache.insert(window[0].clone(), frame());
        let mut full = window[0].clone();
        full.full = true;
        cache.insert(full.clone(), frame());
        assert!(!cache.contains(&full));
        assert_eq!(cache.items.len(), 3);
        assert_eq!(cache.bytes, 48);
        cache.set_window(keys(&[4, 3, 5]));
        assert_eq!(cache.bytes, 32);
    }

    fn job(id: u64, full: bool) -> Job {
        Job {
            entry: entry(3),
            neighbors: vec![entry(4), entry(2)],
            full,
            edge: 100,
            id,
        }
    }

    #[test]
    fn cached_navigation_shares_pixels_without_waiting_for_preload_queue() {
        let shared = Shared::default();
        shared.generation.store(1, Ordering::Relaxed);
        let job = job(1, false);
        assert!(prepare_cache(&shared.cache, &job).is_none());
        let pixels = frame();
        shared
            .cache
            .lock()
            .unwrap()
            .insert(Key::new(&job.entry, false, 100), pixels.clone());
        // Even a busy/locked prefetch mailbox cannot block the cached display path.
        let _busy = shared.prefetch.pending.lock().unwrap();
        let hit = prepare_cache(&shared.cache, &job).unwrap();
        assert!(Arc::ptr_eq(&hit, &pixels));
        assert_eq!(
            hit.buffer.as_bytes().as_ptr(),
            pixels.buffer.as_bytes().as_ptr()
        );
    }

    #[test]
    fn quality_changes_invalidate_previews_and_cache_full_quality_within_budget() {
        let cache = Mutex::new(Cache {
            budget: 32,
            ..Cache::default()
        });
        let preview_job = job(1, false);
        prepare_cache(&cache, &preview_job);
        let old_key = Key::new(&entry(3), false, 100);
        cache.lock().unwrap().insert(old_key.clone(), frame());
        let full_job = job(2, true);
        assert!(prepare_cache(&cache, &full_job).is_none());
        let current = Key::new(&entry(3), true, 100);
        // A window resize shouldn't invalidate a full-resolution image.
        assert!(current == Key::new(&entry(3), true, 4000));
        let mut locked = cache.lock().unwrap();
        locked.insert(old_key.clone(), frame()); // late preview result
        assert!(!locked.contains(&old_key));
        for i in [3, 4, 2] {
            locked.insert(Key::new(&entry(i), true, 100), frame());
        }
        assert!(locked.contains(&current));
        assert!(locked.contains(&Key::new(&entry(4), true, 100)));
        assert!(!locked.contains(&Key::new(&entry(2), true, 100)));
        assert_eq!(locked.bytes, 32);
        drop(locked);
        assert!(prepare_cache(&cache, &full_job).is_some());
        assert!(prepare_cache(&cache, &preview_job).is_none());
    }

    #[test]
    fn late_preload_cannot_replace_new_navigation_or_quality_request() {
        let mailbox = Mailbox::default();
        let generation = AtomicU64::new(2);
        mailbox.put(job(2, true), &generation);
        mailbox.put(job(1, false), &generation);
        let next = mailbox.take(&AtomicBool::new(false)).unwrap();
        assert_eq!(next.id, 2);
        assert!(next.full);
        assert!(mailbox.pending.lock().unwrap().is_none());
    }

    #[test]
    fn cache_tracks_recency_and_file_changes() {
        let entry = ImageEntry {
            metadata: Default::default(),
            path: PathBuf::from("missing"),
            format: "PNG".into(),
            video: false,
            orientation: 1,
            date: None,
            source: "Unknown",
        };
        let key = Key::new(&entry, false, 100);
        let mut cache = Cache::default();
        let frame = Arc::new(Frame {
            buffer: SharedPixelBuffer::new(2, 2),
            original: (2, 2),
            preview: false,
            note: String::new(),
        });
        cache.insert(key.clone(), frame.clone());
        assert!(Arc::ptr_eq(&cache.get(&key).unwrap(), &frame));
        let mut changed = key;
        changed.size = 99;
        assert!(cache.get(&changed).is_none());
        assert_eq!(cache.bytes, 16);
    }
}
