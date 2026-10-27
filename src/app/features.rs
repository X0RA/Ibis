use super::{catalog::Edit, *};
use notify::Watcher;
use slint::{Model, VecModel};
use std::{
    cell::RefCell,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub fn project(w: &AppWindow, s: &mut State) {
    let selected = usize::try_from(w.get_library_selection())
        .ok()
        .and_then(|i| s.entries.get(i))
        .map(|e| e.path.clone());
    let filtered: Vec<_> = s
        .all
        .iter()
        .filter(|e| {
            s.filter
                .matches(e, s.edits.get(&e.path).unwrap_or(&Edit::default()))
        })
        .cloned()
        .collect();
    let visible = filtered
        .iter()
        .map(|e| &e.path)
        .collect::<std::collections::HashSet<_>>();
    s.entries = filtered
        .iter()
        .filter(|e| {
            !s.filter.pair
                || !media::decoder::is_raw(&e.format)
                || !s.pairs.get(&e.path).is_some_and(|p| visible.contains(p))
        })
        .cloned()
        .collect();
    if w.get_group_mode() != 0 {
        s.sort_column = sorting::Column::Date;
        w.set_sort_column(2);
    }
    sort_entries(s, s.sort_column, s.descending);
    s.selected = selected.as_ref().and_then(|p| {
        s.entries.iter().position(|e| &e.path == p).or_else(|| {
            s.pairs
                .get(p)
                .and_then(|q| s.entries.iter().position(|e| &e.path == q))
        })
    });
    if s.selected.is_none() && !s.entries.is_empty() {
        s.selected = Some(0)
    }
    w.set_library_selection(s.selected.map_or(-1, |i| i as i32));
    models(w, s);
}
pub fn models(w: &AppWindow, s: &mut State) {
    let columns = if w.get_grid() { s.columns.max(1) } else { 1 };
    s.positions = s
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.path.clone(), i))
        .collect();
    let mut counts = std::collections::HashMap::<String, i32>::new();
    for entry in &s.entries {
        *counts.entry(group(entry, w.get_group_mode())).or_default() += 1;
    }
    let mut rows = Vec::new();
    let mut bands = Vec::new();
    s.bands.clear();
    let mut y = 0.;
    let mut previous = String::new();
    let mut i = 0;
    while i < s.entries.len() {
        let group = group(&s.entries[i], w.get_group_mode());
        let label = if group != previous {
            group.clone()
        } else {
            String::new()
        };
        previous = group.clone();
        let start = i;
        while i < s.entries.len()
            && i - start < columns
            && group == self::group(&s.entries[i], w.get_group_mode())
        {
            let e = &s.entries[i];
            let edit = s.edits.get(&e.path).cloned().unwrap_or_default();
            rows.push(ImageRow {
                path: e.path.to_string_lossy().as_ref().into(),
                name: e
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .as_ref()
                    .into(),
                directory: e
                    .path
                    .parent()
                    .unwrap_or(std::path::Path::new(""))
                    .to_string_lossy()
                    .as_ref()
                    .into(),
                format: e.format.as_str().into(),
                date: catalog::date_label(e).into(),
                source: catalog::source_label(e).into(),
                year: e
                    .date
                    .map(|_| e.date_label()[..4].to_string())
                    .unwrap_or_default()
                    .into(),
                video: e.video,
                raw: media::decoder::is_raw(&e.format),
                duration: catalog::duration(e).into(),
                index: i as i32,
                band: bands.len() as i32,
                rating: edit.rating,
                rotation: edit.rotation,
                rejected: edit.rejected,
                paired: s.filter.pair && s.pairs.contains_key(&e.path),
                thumbnail: s
                    .thumbs
                    .get(&e.path)
                    .and_then(|p| slint::Image::load_from_path(p).ok())
                    .unwrap_or_default(),
            });
            i += 1;
        }
        let size = if w.get_grid() {
            s.row_height.max(160.)
        } else {
            44.
        } + if label.is_empty() {
            0.
        } else if w.get_grid() {
            42.
        } else {
            36.
        };
        bands.push(LibraryBand {
            count: counts.get(&group).copied().unwrap_or(0),
            label: label.into(),
            cells: Rc::new(VecModel::from(rows[start..i].to_vec())).into(),
            offset: y,
            size,
        });
        s.bands.push((start, i, y, size));
        y += size;
    }
    let selected = s.selected.unwrap_or(0);
    let (start, end) = strip_range(selected, rows.len(), s.strip_count);
    let strip = rows
        .iter()
        .skip(start)
        .take(end - start)
        .cloned()
        .collect::<Vec<_>>();
    w.set_filmstrip(Rc::new(VecModel::from(strip)).into());
    w.set_images(Rc::new(VecModel::from(rows)).into());
    w.set_bands(Rc::new(VecModel::from(bands)).into());
    s.visible_bands = None;
    update_visible_bands(w, s);
    let marks = date_marks(&s.entries, &s.bands, w.get_group_mode());
    s.date_offsets = marks.iter().map(|mark| mark.offset).collect();
    w.set_date_marks(Rc::new(VecModel::from(marks)).into());
    update_date_cursor(w, s);
    w.set_status(
        format!(
            "{} items · {} files in folder",
            s.entries.len(),
            s.all.len()
        )
        .into(),
    );
    w.set_can_previous(selected > 0);
    w.set_can_next(selected + 1 < s.entries.len());
    w.set_photo_position(
        format!(
            "{} / {}",
            if s.entries.is_empty() {
                0
            } else {
                selected + 1
            },
            s.entries.len()
        )
        .into(),
    );
}
fn group(e: &ImageEntry, mode: i32) -> String {
    if mode == 0 {
        String::new()
    } else if e.date.is_none() {
        "Unknown date".into()
    } else {
        e.date
            .unwrap()
            .with_timezone(&chrono::Local)
            .format(match mode {
                2 => "%B %Y",
                3 => "%Y",
                _ => "%-d %B %Y",
            })
            .to_string()
    }
}
pub fn selection(w: &AppWindow, s: &mut State, e: &ImageEntry) {
    let edit = s.edits.get(&e.path).cloned().unwrap_or_default();
    w.set_info(catalog::info(e).into());
    w.set_details(
        Rc::new(VecModel::from(
            catalog::detail_rows(e)
                .into_iter()
                .map(|(label, value)| DetailRow {
                    label: label.into(),
                    value: value.into(),
                })
                .collect::<Vec<_>>(),
        ))
        .into(),
    );
    w.set_media_kind(
        if s.filter.pair && s.pairs.contains_key(&e.path) {
            format!(
                "{} · paired",
                if media::decoder::is_raw(&e.format) {
                    "RAW"
                } else {
                    e.format.as_str()
                }
            )
        } else {
            String::new()
        }
        .into(),
    );
    w.set_rotation(edit.rotation);
    w.set_rating(edit.rating);
    w.set_rejected(edit.rejected);
    w.set_has_pair(s.filter.pair && s.pairs.contains_key(&e.path));
    w.set_action_status("".into());
    position(w, s);
    let (start, end) = strip_range(s.selected.unwrap_or(0), s.entries.len(), s.strip_count);
    request(s, start, end);
}
fn position(w: &AppWindow, s: &State) {
    let selected = s.selected.unwrap_or(0);
    let rows = w.get_images();
    let (start, end) = strip_range(selected, rows.row_count(), s.strip_count);
    let strip = (start..end)
        .filter_map(|i| rows.row_data(i))
        .collect::<Vec<_>>();
    w.set_filmstrip(Rc::new(VecModel::from(strip)).into());
    w.set_can_previous(selected > 0);
    w.set_can_next(selected + 1 < s.entries.len());
    w.set_photo_position(
        format!(
            "{} / {}",
            if s.entries.is_empty() {
                0
            } else {
                selected + 1
            },
            s.entries.len()
        )
        .into(),
    );
}
/// Update one tile in place; decoding a thumbnail must not rebuild the entire library.
fn update_row(w: &AppWindow, s: &State, path: &std::path::Path) {
    let Some(&index) = s.positions.get(path) else {
        return;
    };
    let images = w.get_images();
    let Some(mut row) = images.row_data(index) else {
        return;
    };
    row.thumbnail = s
        .thumbs
        .get(path)
        .and_then(|p| slint::Image::load_from_path(p).ok())
        .unwrap_or_default();
    if let Some(edit) = s.edits.get(path) {
        row.rotation = edit.rotation;
        row.rating = edit.rating;
        row.rejected = edit.rejected;
    }
    images.set_row_data(index, row.clone());
    if let Some(band) = w.get_bands().row_data(row.band as usize)
        && let Some(first) = band.cells.row_data(0)
    {
        band.cells
            .set_row_data(index - first.index as usize, row.clone());
    }
    let strip = w.get_filmstrip();
    for i in 0..strip.row_count() {
        if strip.row_data(i).is_some_and(|e| e.index == index as i32) {
            strip.set_row_data(i, row.clone());
        }
    }
}
fn request(s: &mut State, start: usize, end: usize) {
    s.wanted = s
        .entries
        .iter()
        .skip(start)
        .take(end.saturating_sub(start).min(100))
        .map(|e| e.path.clone())
        .collect();
    if let Some(tx) = &s.thumb_requests {
        let entries = s
            .entries
            .iter()
            .skip(start)
            .take(end.saturating_sub(start).min(100))
            .filter(|e| {
                !s.thumbs.get(&e.path).is_some_and(|p| p.exists())
                    && !s.failed_thumbs.contains(&e.path)
            })
            .cloned()
            .collect::<Vec<_>>();
        if !entries.is_empty() {
            let _ = tx.send((s.scan_id, entries));
        }
    }
}
pub struct Runtime {
    _timer: slint::Timer,
}
pub fn install(
    w: &AppWindow,
    state: Arc<Mutex<State>>,
    viewer: Rc<viewer::Viewer>,
    player: Option<Rc<video::Player>>,
) -> Runtime {
    let operation = Rc::new(std::cell::Cell::new(false));
    state.lock().unwrap().filter.pair = true;
    let config_path = crate::paths::config_dir().join("settings.json");
    match settings::Settings::load(&config_path) {
        Ok(settings) => {
            state.lock().unwrap().settings = settings;
            w.set_write_metadata(settings.write_metadata);
        }
        Err(error) => w.set_settings_status(error.into()),
    }
    {
        let weak = w.as_weak();
        let state = state.clone();
        let operation = operation.clone();
        w.on_change_metadata_mode(move |enabled| {
            let Some(w) = weak.upgrade() else { return };
            let mut s = state.lock().unwrap();
            if s.metadata_busy || s.scanning || operation.get() {
                w.set_settings_status("Wait for the current operation to finish".into());
                return;
            }
            let settings = settings::Settings {
                write_metadata: enabled,
            };
            if let Err(error) = settings.save(&config_path) {
                w.set_settings_status(format!("Could not save settings: {error}").into());
                return;
            }
            s.settings = settings;
            w.set_write_metadata(enabled);
            w.set_settings_status("Saved".into());
            s.edits = s
                .all
                .iter()
                .map(|e| (e.path.clone(), Edit::load(e, enabled)))
                .collect();
            project(&w, &mut s);
            if let Some(entry) = s
                .active
                .as_ref()
                .and_then(|path| s.all.iter().find(|e| &e.path == path))
                .cloned()
            {
                selection(&w, &mut s, &entry);
            }
        });
    }
    {
        let weak = w.as_weak();
        let state = state.clone();
        w.on_filmstrip_capacity(move |count| {
            if let Some(w) = weak.upgrade() {
                let mut s = state.lock().unwrap();
                s.strip_count = (count as usize).clamp(1, 100);
                position(&w, &s);
                if w.get_viewer_open() {
                    let (start, end) =
                        strip_range(s.selected.unwrap_or(0), s.entries.len(), s.strip_count);
                    request(&mut s, start, end);
                }
            }
        });
    }
    let worker = thumbnails::Worker::new();
    state.lock().unwrap().thumb_requests = Some(worker.requests.clone());
    {
        let weak = w.as_weak();
        let s = state.clone();
        w.on_apply_filter(move || {
            if let Some(w) = weak.upgrade() {
                let f = catalog::Filter {
                    kind: w.get_type_filter(),
                    rating: w.get_rating_filter(),
                    camera: w.get_camera_filter().to_string(),
                    from: w.get_date_from().trim().into(),
                    to: w.get_date_to().trim().into(),
                    pair: w.get_pair_files(),
                };
                if let Err(e) = f.validate() {
                    w.set_status(e.into());
                    return;
                }
                let mut s = s.lock().unwrap();
                s.filter = f;
                project(&w, &mut s);
            }
        });
    }
    {
        let weak = w.as_weak();
        let s = state.clone();
        w.on_layout_changed(move |columns, height| {
            if let Some(w) = weak.upgrade() {
                let mut s = s.lock().unwrap();
                let columns = columns.max(1) as usize;
                let height = height.clamp(44., 1000.);
                if s.columns == columns && (s.row_height - height).abs() < 0.01 {
                    return;
                }
                s.columns = columns;
                s.row_height = height;
                models(&w, &mut s);
                drop(s);
                if !w.get_viewer_open() {
                    w.invoke_focus_library();
                }
            }
        });
    }
    {
        let s = state.clone();
        let weak = w.as_weak();
        w.on_request_thumbnails(move |y, h| {
            let Some(w) = weak.upgrade() else { return };
            if w.get_viewer_open() {
                return;
            }
            let mut s = s.lock().unwrap();
            s.viewport_top = y.max(0.);
            s.viewport_height = h.max(1.);
            update_visible_bands(&w, &mut s);
            let range = s.visible_bands.clone().unwrap_or(0..0);
            if let (Some(first), Some(last)) = (
                s.bands.get(range.start),
                range.end.checked_sub(1).and_then(|i| s.bands.get(i)),
            ) {
                let (start, end) = (first.0, last.1);
                request(&mut s, start, end);
            }
        });
    }
    {
        let weak = w.as_weak();
        let s = state.clone();
        let operation = operation.clone();
        w.on_set_rating(move |rating| {
            if let Some(w) = weak.upgrade() {
                if operation.get() {
                    notice(&w, Err("File operation in progress".into()));
                    return;
                }
                edit(&w, &s, true, |e| {
                    e.rating = rating.clamp(0, 5);
                });
            }
        });
    }
    let undo = Rc::new(RefCell::new(Vec::<file_actions::TrashUndo>::new()));
    let clipboard = Rc::new(RefCell::new(None::<arboard::Clipboard>));
    {
        let weak = w.as_weak();
        let s = state.clone();
        let undo = undo.clone();
        let busy = operation.clone();
        let clipboard = clipboard.clone();
        w.on_action(move |action| {
            let Some(w) = weak.upgrade() else { return };
            let action = action.as_str();
            if busy.get() || s.lock().unwrap().metadata_busy {
                w.set_action_status("File operation in progress…".into());
                return;
            }
            if action == "rotate" {
                edit(&w, &s, false, |e| e.rotation = (e.rotation + 1) % 4);
                w.invoke_focus_viewer();
                return;
            }
            if action == "reject" {
                edit(&w, &s, true, |e| e.rejected = !e.rejected);
                w.invoke_focus_viewer();
                return;
            }
            let entry = {
                let s = s.lock().unwrap();
                if w.get_viewer_open() {
                    s.active
                        .as_ref()
                        .and_then(|p| s.all.iter().find(|e| &e.path == p))
                        .cloned()
                } else {
                    usize::try_from(w.get_library_selection())
                        .ok()
                        .and_then(|i| s.entries.get(i))
                        .cloned()
                }
            };
            if action == "pair" {
                let mut s = s.lock().unwrap();
                let paired = s
                    .active
                    .as_ref()
                    .and_then(|p| s.pairs.get(p))
                    .and_then(|p| s.all.iter().find(|e| &e.path == p))
                    .cloned();
                if let Some(entry) = paired {
                    s.active = Some(entry.path.clone());
                    selection(&w, &mut s, &entry);
                    w.set_photo_title(
                        entry
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .as_ref()
                            .into(),
                    );
                    w.set_photo_revision(w.get_photo_revision().wrapping_add(1));
                    viewer.open(&w, &entry, vec![], w.get_full_quality());
                    w.invoke_focus_viewer();
                }
                return;
            }
            if action == "copy-path" || action == "copy-image" {
                let Some(entry) = entry else { return };
                let result = (|| -> Result<(), String> {
                    let mut cb = clipboard.borrow_mut();
                    if cb.is_none() {
                        *cb = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?)
                    }
                    let cb = cb.as_mut().unwrap();
                    if action == "copy-path" {
                        cb.set_text(entry.path.to_string_lossy())
                            .map_err(|e| e.to_string())
                    } else {
                        let pixels = w.get_photo().to_rgba8().ok_or("Image is still loading")?;
                        let im = image::RgbaImage::from_raw(
                            pixels.width(),
                            pixels.height(),
                            pixels.as_bytes().to_vec(),
                        )
                        .ok_or("Invalid image")?;
                        let im = match w.get_rotation() {
                            1 => image::imageops::rotate90(&im),
                            2 => image::imageops::rotate180(&im),
                            3 => image::imageops::rotate270(&im),
                            _ => im,
                        };
                        cb.set_image(arboard::ImageData {
                            width: im.width() as usize,
                            height: im.height() as usize,
                            bytes: std::borrow::Cow::Owned(im.into_raw()),
                        })
                        .map_err(|e| e.to_string())
                    }
                })();
                notice(
                    &w,
                    result.map(|_| {
                        if action == "copy-path" {
                            "Copied path"
                        } else {
                            "Copied image"
                        }
                        .to_string()
                    }),
                );
                w.invoke_focus_viewer();
                return;
            }
            if action == "reveal" {
                if let Some(entry) = entry {
                    notice(
                        &w,
                        file_actions::reveal(&entry.path).map(|_| "Opened in file manager".into()),
                    );
                }
                return;
            }
            if action == "undo" {
                let Some(items) = undo.borrow_mut().pop() else {
                    notice(&w, Ok("Nothing to undo".into()));
                    return;
                };
                // Restore collision errors never overwrite an existing file.
                let result = file_actions::undo(items.clone());
                if result.is_err() {
                    undo.borrow_mut().push(items);
                }
                notice(&w, result.map(|_| "Restored from trash".into()));
                super::start_scan(&w, PathBuf::from(w.get_folder().as_str()), s.clone());
                return;
            }
            if action == "trash" {
                let Some(entry) = entry else { return };
                w.set_slideshow(false);
                if w.get_viewer_open() {
                    w.invoke_close_photo();
                }
                busy.set(true);
                let weak = w.as_weak();
                let s = s.clone();
                let path = entry.path;
                let undo = undo.clone();
                let busy = busy.clone();
                // The filesystem work runs off the UI thread; its result owns the undo record.
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(file_actions::trash_file(&path));
                });
                poll_operation(weak, rx, move |w, result| {
                    busy.set(false);
                    match result {
                        Ok(items) => {
                            undo.borrow_mut().push(items);
                            notice(&w, Ok("Moved to system trash · Ctrl+Z to undo".into()));
                        }
                        Err(e) => notice(&w, Err(e)),
                    }
                    super::start_scan(&w, PathBuf::from(w.get_folder().as_str()), s.clone());
                });
                return;
            }
            if action == "move-rejected" {
                let files = {
                    let s = s.lock().unwrap();
                    s.all
                        .iter()
                        .filter(|e| s.edits.get(&e.path).is_some_and(|x| x.rejected))
                        .map(|e| e.path.clone())
                        .collect::<Vec<_>>()
                };
                if files.is_empty() {
                    notice(&w, Ok("No rejected files".into()));
                    return;
                }
                busy.set(true);
                let weak = w.as_weak();
                let s = s.clone();
                let busy = busy.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let Some(folder) = rfd::FileDialog::new()
                        .set_title(format!("Move {} rejected files to folder", files.len()))
                        .pick_folder()
                    else {
                        let _ = tx.send(Ok("Move cancelled".into()));
                        return;
                    };
                    let mut moved = 0;
                    let mut errors = Vec::new();
                    for source in files {
                        match file_actions::move_file(&source, &folder) {
                            Ok(target) => {
                                let old = catalog::data_dir()
                                    .join(format!("{}.json", catalog::key(&source)));
                                let new = catalog::data_dir()
                                    .join(format!("{}.json", catalog::key(&target)));
                                if old.exists() {
                                    let _ = std::fs::rename(old, new);
                                }
                                moved += 1;
                            }
                            Err(e) => errors.push(format!("{}: {e}", source.display())),
                        }
                    }
                    let _ = tx.send(Ok(format!(
                        "Moved {moved} files{}",
                        if errors.is_empty() {
                            String::new()
                        } else {
                            format!(" · {} failures: {}", errors.len(), errors.join("; "))
                        }
                    )));
                });
                poll_operation(weak, rx, move |w, result| {
                    busy.set(false);
                    notice(&w, result);
                    super::start_scan(&w, PathBuf::from(w.get_folder().as_str()), s.clone());
                });
            }
        });
    }
    let weak = w.as_weak();
    let s = state.clone();
    let dirty = Arc::new(AtomicU64::new(0));
    let change = dirty.clone();
    let ignored = [
        crate::paths::cache_dir(),
        crate::paths::data_dir(),
        crate::paths::data_base().join("Trash"),
    ];
    let mut watcher =
        notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
            if event.is_ok_and(|e| {
                (e.paths.is_empty()
                    || e.paths
                        .iter()
                        .any(|path| !ignored.iter().any(|root| path.starts_with(root))))
                    && matches!(
                        e.kind,
                        notify::EventKind::Create(_)
                            | notify::EventKind::Remove(_)
                            | notify::EventKind::Modify(_)
                    )
            }) {
                change.fetch_add(1, Ordering::Relaxed);
            }
        })
        .ok();
    let mut watched = PathBuf::new();
    let mut last_dirty = 0;
    let mut changed_at = Instant::now();
    let mut pending_change = false;
    let mut revision = -1;
    let mut started = Instant::now();
    let mut showing = false;
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(100),
        move || {
            let Some(w) = weak.upgrade() else { return };

            {
                let mut s = s.lock().unwrap();
                for (generation, path, result) in worker.results.try_iter().take(24) {
                    if generation != s.scan_id || !s.all.iter().any(|e| e.path == path) {
                        continue;
                    }
                    match result {
                        Ok(p) => {
                            s.thumbs.insert(path.clone(), p);
                            update_row(&w, &s, &path);
                        }
                        Err(_) => {
                            s.failed_thumbs.insert(path);
                        }
                    }
                }
                if s.thumbs.len() > 256 {
                    let keep = s.wanted.clone();
                    let removed = s
                        .thumbs
                        .keys()
                        .filter(|p| !keep.contains(*p))
                        .cloned()
                        .collect::<Vec<_>>();
                    for path in removed {
                        s.thumbs.remove(&path);
                        update_row(&w, &s, &path);
                    }
                }
            }
            let folder = PathBuf::from(w.get_folder().as_str());
            if folder != watched && folder.is_dir() {
                if let Some(watcher) = watcher.as_mut() {
                    if !watched.as_os_str().is_empty() {
                        let _ = watcher.unwatch(&watched);
                    }
                    let mode = if s.lock().unwrap().recursive {
                        notify::RecursiveMode::Recursive
                    } else {
                        notify::RecursiveMode::NonRecursive
                    };
                    if let Err(e) = watcher.watch(&folder, mode) {
                        w.set_action_status(format!("Folder watching unavailable: {e}").into());
                    }
                }
                watched = folder.clone();
            }
            let now = dirty.load(Ordering::Relaxed);
            if now != last_dirty {
                last_dirty = now;
                changed_at = Instant::now();
                pending_change = true;
            }
            if pending_change
                && changed_at.elapsed() > Duration::from_millis(700)
                && !s.lock().unwrap().scanning
                && !s.lock().unwrap().metadata_busy
                && !operation.get()
            {
                pending_change = false;
                super::start_scan(&w, folder, s.clone());
            }
            if revision != w.get_photo_revision() || showing != w.get_slideshow() {
                revision = w.get_photo_revision();
                showing = w.get_slideshow();
                started = Instant::now();
                if showing && w.get_is_video() {
                    w.invoke_video_pause(false);
                }
            }
            w.set_slide_progress(if w.get_slideshow() && w.get_viewer_open() {
                if w.get_is_video() {
                    (w.get_video_position() / w.get_video_duration().max(0.001)).clamp(0., 1.)
                } else if w.get_photo_loading() {
                    0.
                } else {
                    (started.elapsed().as_secs_f32() / w.get_slide_seconds().max(1) as f32)
                        .clamp(0., 1.)
                }
            } else {
                0.
            });
            if w.get_slideshow() && w.get_viewer_open() {
                let ready = if w.get_is_video() {
                    w.get_video_ended()
                        || !w.get_video_error().is_empty()
                            && started.elapsed().as_secs() >= w.get_slide_seconds().max(1) as u64
                } else {
                    if w.get_photo_loading() {
                        started = Instant::now();
                    }
                    !w.get_photo_loading()
                        && started.elapsed().as_secs() >= w.get_slide_seconds().max(1) as u64
                };
                if ready {
                    if w.get_can_next() {
                        w.invoke_navigate(1);
                    } else {
                        w.set_slideshow(false);
                    }
                }
            }
        },
    );
    let _ = player;
    Runtime { _timer: timer }
}
fn edit(
    w: &AppWindow,
    state: &Arc<Mutex<State>>,
    metadata_change: bool,
    change: impl FnOnce(&mut Edit),
) {
    let mut s = state.lock().unwrap();
    if s.metadata_busy {
        notice(
            w,
            Err("Wait for the current file operation to finish".into()),
        );
        return;
    }
    let Some(path) = s.active.clone() else { return };
    let old = s.edits.get(&path).cloned().unwrap_or_default();
    let mut edited = old.clone();
    change(&mut edited);
    if metadata_change && s.settings.write_metadata {
        if edited.rating != old.rating {
            edited.rejected = false;
        }
        if edited.rejected {
            edited.rating = 0;
        }
        let value = if edited.rejected { -1 } else { edited.rating };
        s.metadata_busy = true;
        w.set_edit_busy(true);
        let (tx, rx) = std::sync::mpsc::channel();
        let file = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(metadata::write_rating(&file, value));
        });
        let state = state.clone();
        poll_operation(w.as_weak(), rx, move |w, result| {
            let mut s = state.lock().unwrap();
            s.metadata_busy = false;
            w.set_edit_busy(false);
            match result {
                Ok(()) => {
                    s.metadata_epoch += 1;
                    let epoch = s.metadata_epoch;
                    s.metadata_writes.insert(path.clone(), (epoch, value));
                    for entry in &mut s.all {
                        if entry.path == path {
                            entry.metadata["Rating"] = serde_json::json!(value);
                            entry.metadata["XMP-xmp:Rating"] = serde_json::json!(value);
                        }
                    }
                    for entry in &mut s.entries {
                        if entry.path == path {
                            entry.metadata["Rating"] = serde_json::json!(value);
                            entry.metadata["XMP-xmp:Rating"] = serde_json::json!(value);
                        }
                    }
                    if s.all.iter().any(|e| e.path == path) {
                        s.edits.insert(path.clone(), edited.clone());
                        update_row(&w, &s, &path);
                    }
                    if s.active.as_ref() == Some(&path) {
                        w.set_rating(edited.rating);
                        w.set_rejected(edited.rejected);
                    }
                    notice(&w, Ok("Saved to file metadata".into()));
                }
                Err(error) => notice(&w, Err(error)),
            }
        });
    } else {
        let result = if metadata_change {
            edited.save(&path)
        } else {
            edited.save_rotation(&path)
        };
        if let Err(error) = result {
            notice(w, Err(error));
            return;
        }
        w.set_rating(edited.rating);
        w.set_rejected(edited.rejected);
        w.set_rotation(edited.rotation);
        if !metadata_change && w.get_is_video() {
            w.invoke_video_rotate(edited.rotation);
        }
        s.edits.insert(path.clone(), edited);
        update_row(w, &s, &path);
    }
}

fn notice(w: &AppWindow, result: Result<String, String>) {
    let message = match result {
        Ok(s) => s,
        Err(e) => format!("Could not complete action: {e}"),
    };
    let id = w.get_notice_id().wrapping_add(1);
    w.set_notice_id(id);
    w.set_action_status(message.into());
    let weak = w.as_weak();
    slint::Timer::single_shot(Duration::from_secs(4), move || {
        if let Some(w) = weak.upgrade()
            && w.get_notice_id() == id
        {
            w.set_action_status("".into());
        }
    });
}
fn poll_operation<T: 'static>(
    weak: slint::Weak<AppWindow>,
    rx: std::sync::mpsc::Receiver<T>,
    done: impl FnOnce(AppWindow, T) + 'static,
) {
    slint::Timer::single_shot(Duration::from_millis(50), move || match rx.try_recv() {
        Ok(value) => {
            if let Some(w) = weak.upgrade() {
                done(w, value)
            }
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => poll_operation(weak, rx, done),
        Err(_) => {}
    });
}

/// Rating uses the app's non-destructive edits, rather than only embedded metadata.
pub fn sort_entries(s: &mut State, column: sorting::Column, descending: bool) {
    if matches!(column, sorting::Column::Rating) {
        let edits = &s.edits;
        s.entries.sort_by(|a, b| {
            let order = edits
                .get(&a.path)
                .map_or(0, |e| e.rating)
                .cmp(&edits.get(&b.path).map_or(0, |e| e.rating));
            (if descending { order.reverse() } else { order }).then_with(|| a.path.cmp(&b.path))
        });
    } else {
        sorting::sort(&mut s.entries, column, descending);
    }
}

fn strip_range(selected: usize, len: usize, count: usize) -> (usize, usize) {
    let count = if count == 0 { 7 } else { count }.min(len);
    let start = selected
        .saturating_sub(count / 2)
        .min(len.saturating_sub(count));
    (start, start + count)
}

#[cfg(test)]
mod strip_tests {
    use super::strip_range;
    #[test]
    fn strip_fills_available_slots_at_both_ends() {
        assert_eq!(strip_range(0, 248, 19), (0, 19));
        assert_eq!(strip_range(247, 248, 19), (229, 248));
        assert_eq!(strip_range(100, 248, 19), (91, 110));
        assert_eq!(strip_range(2, 3, 19), (0, 3));
        assert_eq!(strip_range(0, 0, 19), (0, 0));
    }
}

/// Use the catalogue's exact geometry instead of an estimated average row height.
fn visible_band_range(
    bands: &[(usize, usize, f32, f32)],
    top: f32,
    height: f32,
) -> std::ops::Range<usize> {
    let start = bands.partition_point(|(_, _, y, h)| y + h < top - 200.);
    let end = bands.partition_point(|(_, _, y, _)| *y <= top + height + 200.);
    start..end.max(start)
}
fn update_visible_bands(w: &AppWindow, s: &mut State) {
    update_date_cursor(w, s);
    let range = visible_band_range(&s.bands, s.viewport_top, s.viewport_height.max(1.));
    if s.visible_bands.as_ref() == Some(&range) {
        return;
    }
    let bands = w.get_bands();
    let visible = range
        .clone()
        .filter_map(|i| bands.row_data(i))
        .collect::<Vec<_>>();
    s.visible_bands = Some(range);
    w.set_visible_bands(Rc::new(VecModel::from(visible)).into());
}

#[cfg(test)]
mod viewport_tests {
    use super::visible_band_range;
    #[test]
    fn exact_geometry_handles_headers_large_jumps_and_empty_catalogues() {
        let bands: Vec<_> = (0..1000)
            .scan(0., |offset, i| {
                let height = if i % 7 == 0 { 80. } else { 44. };
                let band = (i, i + 1, *offset, height);
                *offset += height;
                Some(band)
            })
            .collect();
        for top in [0., 133., 8000., 40000.] {
            let expected = bands
                .iter()
                .enumerate()
                .filter(|(_, (_, _, y, h))| y + h >= top - 200. && *y <= top + 800. + 200.)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            assert_eq!(
                visible_band_range(&bands, top, 800.).collect::<Vec<_>>(),
                expected
            );
        }
        assert_eq!(visible_band_range(&[], 0., 800.), 0..0);
    }
}

fn date_marks(
    entries: &[ImageEntry],
    bands: &[(usize, usize, f32, f32)],
    mode: i32,
) -> Vec<DateMark> {
    if !matches!(mode, 2 | 3) {
        return Vec::new();
    }
    let mut marks = Vec::new();
    let mut previous_year = String::new();
    let mut previous_month = String::new();
    for &(start, _, offset, _) in bands {
        let Some(entry) = entries.get(start) else {
            continue;
        };
        let date = entry.date.map(|date| date.with_timezone(&chrono::Local));
        let year = date
            .map(|date| date.format("%Y").to_string())
            .unwrap_or_else(|| "Unknown".into());
        if year != previous_year {
            marks.push(DateMark {
                label: year.as_str().into(),
                title: year.as_str().into(),
                offset,
                index: start as i32,
                major: true,
            });
            previous_year = year;
        }
        if mode == 2
            && let Some(date) = date
        {
            let month = date.format("%B %Y").to_string();
            if month != previous_month {
                marks.push(DateMark {
                    label: date.format("%b").to_string().into(),
                    title: month.as_str().into(),
                    offset,
                    index: start as i32,
                    major: false,
                });
                previous_month = month;
            }
        }
    }
    marks
}
fn update_date_cursor(w: &AppWindow, s: &State) {
    let offsets = &s.date_offsets;
    if offsets.is_empty() {
        w.set_date_cursor(0.);
        return;
    }
    let total = s.bands.last().map_or(0., |b| b.2 + b.3);
    let at_end = s.viewport_top > 0. && s.viewport_top + s.viewport_height >= total - 1.;
    let index = if at_end {
        offsets.len() - 1
    } else {
        offsets
            .partition_point(|y| *y <= s.viewport_top)
            .saturating_sub(1)
    };
    let fraction = offsets.get(index + 1).map_or(0., |next| {
        ((s.viewport_top - offsets[index]) / (next - offsets[index]).max(1.)).clamp(0., 1.)
    });
    w.set_date_cursor(index as f32 + fraction);
}

#[cfg(test)]
mod date_rail_tests {
    use super::*;
    fn entry(date: Option<&str>) -> ImageEntry {
        ImageEntry {
            path: "test.jpg".into(),
            format: "JPEG".into(),
            orientation: 1,
            video: false,
            date: date.map(|d| {
                chrono::DateTime::parse_from_rfc3339(d)
                    .unwrap()
                    .with_timezone(&chrono::Utc)
            }),
            source: "EXIF capture date",
            metadata: serde_json::Value::Null,
        }
    }
    #[test]
    fn months_are_nested_under_years_with_exact_jump_targets() {
        let entries = vec![
            entry(Some("2023-02-12T12:00:00Z")),
            entry(Some("2023-06-12T12:00:00Z")),
            entry(Some("2024-03-12T12:00:00Z")),
            entry(None),
        ];
        let bands = vec![
            (0, 1, 0., 80.),
            (1, 2, 80., 80.),
            (2, 3, 160., 80.),
            (3, 4, 240., 80.),
        ];
        let marks = date_marks(&entries, &bands, 2);
        assert_eq!(
            marks.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(),
            ["2023", "Feb", "Jun", "2024", "Mar", "Unknown"]
        );
        assert_eq!(marks[2].offset, 80.);
        assert_eq!(marks[2].index, 1);
        assert!(!marks[2].major);
        assert!(marks[3].major);
        let years = date_marks(&entries, &bands, 3);
        assert_eq!(
            years.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(),
            ["2023", "2024", "Unknown"]
        );
        assert_eq!(group(&entries[0], 3), "2023");
        assert!(date_marks(&entries, &bands, 0).is_empty());
        assert!(date_marks(&entries, &bands, 1).is_empty());
        let reversed = vec![entries[2].clone(), entries[1].clone(), entries[0].clone()];
        let descending = date_marks(&reversed, &bands[..3], 2);
        assert_eq!(
            descending
                .iter()
                .map(|m| m.label.as_str())
                .collect::<Vec<_>>(),
            ["2024", "Mar", "2023", "Jun", "Feb"]
        );
    }
}
