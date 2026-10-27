mod catalog;
mod features;
mod file_actions;
mod folder_dialog;
mod metadata;
mod settings;
mod sorting;
mod thumbnails;
mod video;
mod viewer;

use crate::media::{self, ImageEntry};
use slint::ComponentHandle;
use std::{path::PathBuf, rc::Rc};

slint::include_modules!();

#[derive(Default)]
struct State {
    settings: settings::Settings,
    metadata_busy: bool,
    metadata_epoch: u64,
    metadata_writes: std::collections::HashMap<PathBuf, (u64, i32)>,
    entries: Vec<ImageEntry>,
    all: Vec<ImageEntry>,
    edits: std::collections::HashMap<PathBuf, catalog::Edit>,
    pairs: std::collections::HashMap<PathBuf, PathBuf>,
    positions: std::collections::HashMap<PathBuf, usize>,
    thumbs: std::collections::HashMap<PathBuf, PathBuf>,
    wanted: std::collections::HashSet<PathBuf>,
    failed_thumbs: std::collections::HashSet<PathBuf>,
    thumb_requests: Option<std::sync::mpsc::Sender<(u64, Vec<ImageEntry>)>>,
    bands: Vec<(usize, usize, f32, f32)>,
    date_offsets: Vec<f32>,
    visible_bands: Option<std::ops::Range<usize>>,
    viewport_top: f32,
    viewport_height: f32,
    columns: usize,
    row_height: f32,
    strip_count: usize,
    filter: catalog::Filter,
    active: Option<PathBuf>,
    pending_open: Option<PathBuf>,
    recursive: bool,
    scan_id: u64,
    scanning: bool,

    selected: Option<usize>,
    sort_column: sorting::Column,
    descending: bool,
}

pub fn run(directory: Option<PathBuf>, cache_bytes: usize) -> Result<(), slint::PlatformError> {
    slint::BackendSelector::new()
        .renderer_name("femtovg".into())
        .require_opengl()
        .select()?;
    let window = AppWindow::new()?;
    let player = match video::Player::new(&window) {
        Ok(player) => Some(Rc::new(player)),
        Err(error) => {
            window.set_video_error(error.into());
            None
        }
    };
    if let Some(player) = &player {
        let p = player.clone();
        window.on_video_pause(move |v| p.pause(v));
        let p = player.clone();
        window.on_video_seek(move |v| p.seek(v));
        let p = player.clone();
        window.on_video_set_volume(move |v| p.volume(v));
        let p = player.clone();
        window.on_video_mute(move |v| p.mute(v));
        let p = player.clone();
        window.on_video_frame_step(move |forward| p.frame_step(forward));
        let p = player.clone();
        window.on_video_rotate(move |turns| p.rotate(turns));
    }
    {
        let weak = window.as_weak();
        window.on_toggle_fullscreen(move || {
            if let Some(window) = weak.upgrade() {
                window
                    .window()
                    .set_fullscreen(!window.window().is_fullscreen());
            }
        });
        let weak = window.as_weak();
        window.on_viewer_escape(move || {
            if let Some(window) = weak.upgrade() {
                if window.window().is_fullscreen() {
                    window.window().set_fullscreen(false);
                } else {
                    window.invoke_close_photo();
                }
            }
        });
    }
    let state = std::sync::Arc::new(std::sync::Mutex::new(State::default()));
    let viewer = Rc::new(viewer::Viewer::new(&window, cache_bytes));
    let _features = features::install(&window, state.clone(), viewer.clone(), player.clone());
    {
        let weak = window.as_weak();
        let state = state.clone();
        let viewer = viewer.clone();
        let player = player.clone();
        window.on_jump_item(move |last| {
            let length = state.lock().unwrap().entries.len();
            if length == 0 {
                return;
            }
            if let Some(window) = weak.upgrade() {
                select(
                    &window,
                    &state,
                    &viewer,
                    player.as_deref(),
                    if last { length as i32 - 1 } else { 0 },
                );
            }
        });
    }
    {
        let weak = window.as_weak();
        let state = state.clone();
        window.on_sort_library(move |index| {
            let Some(column) = sorting::Column::from_index(index) else {
                return;
            };
            let Some(window) = weak.upgrade() else {
                return;
            };
            if window.get_busy() || window.get_viewer_open() {
                return;
            }
            let mut state = state.lock().unwrap();
            let selected = usize::try_from(window.get_library_selection())
                .ok()
                .and_then(|i| state.entries.get(i))
                .map(|entry| entry.path.clone());
            state.descending = state.sort_column.index() == index && !state.descending;
            state.sort_column = column;
            let descending = state.descending;
            features::sort_entries(&mut state, column, descending);
            state.selected =
                selected.and_then(|path| state.entries.iter().position(|entry| entry.path == path));
            window.set_library_selection(state.selected.map_or(-1, |i| i as i32));
            window.set_sort_column(index);
            window.set_sort_descending(descending);
            window.set_group_mode(0);
            show_rows(&window, &mut state);
        });
    }

    {
        let weak = window.as_weak();
        let state = state.clone();
        let viewer = viewer.clone();
        let player = player.clone();
        window.on_open_photo(move |index| {
            if let Some(window) = weak.upgrade() {
                select(&window, &state, &viewer, player.as_deref(), index);
            }
        });
    }
    {
        let weak = window.as_weak();
        let state = state.clone();
        let viewer = viewer.clone();
        let player = player.clone();
        window.on_navigate(move |delta| {
            let index = state.lock().unwrap().selected.map(|i| i as i32 + delta);
            if let (Some(window), Some(index)) = (weak.upgrade(), index) {
                select(&window, &state, &viewer, player.as_deref(), index);
            }
        });
    }
    {
        let weak = window.as_weak();
        let state = state.clone();
        let viewer = viewer.clone();
        window.on_set_full_quality(move |full| {
            if let Some(window) = weak.upgrade() {
                window.set_full_quality(full);
                let state = state.lock().unwrap();
                if let Some(entry) = state
                    .active
                    .as_ref()
                    .and_then(|p| state.all.iter().find(|e| &e.path == p))
                    .filter(|e| !e.video)
                {
                    let index = state.selected.unwrap_or(0);
                    viewer.open(
                        &window,
                        entry,
                        viewer::neighbors(&state.entries, index, false),
                        full,
                    );
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        let viewer = viewer.clone();
        let player = player.clone();
        let state = state.clone();
        window.on_close_photo(move || {
            viewer.cancel();
            if let Some(player) = &player {
                player.stop();
            }
            if let Some(window) = weak.upgrade() {
                window.window().set_fullscreen(false);
                window.set_slideshow(false);
                window.set_viewer_open(false);
                {
                    let mut state = state.lock().unwrap();
                    state.active = None;
                    features::project(&window, &mut state);
                }
                window.invoke_focus_library();
                window.set_photo(Default::default());
                window.set_video_texture(Default::default());
                window.set_photo_loading(false);
            }
        });
    }
    let weak = window.as_weak();
    let picker_state = state.clone();
    let picker_viewer = viewer.clone();
    let picker_player = player.clone();
    window.on_choose_folder(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        picker_viewer.cancel();
        if let Some(player) = &picker_player {
            player.stop();
        }
        window.set_video_texture(Default::default());
        window.window().set_fullscreen(false);
        window.set_viewer_open(false);
        let state = picker_state.clone();
        window.set_scan_completed(0);
        window.set_scan_total(-1);
        window.set_busy(true);
        window.set_status("Selecting folder…".into());
        let weak = weak.clone();
        std::thread::spawn(move || {
            let result = folder_dialog::choose_folder();
            let _ = weak.upgrade_in_event_loop(move |window| match result {
                Ok(Some(directory)) => {
                    state.lock().unwrap().recursive = true;
                    start_scan(&window, directory, state.clone());
                }
                Ok(None) => {
                    window.set_busy(false);
                    window.set_status("Cancelled".into());
                }
                Err(error) => {
                    window.set_busy(false);
                    window.set_status(error.into());
                }
            });
        });
    });
    if let Some(path) = directory {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        if path.is_file() {
            state.lock().unwrap().pending_open = Some(path.clone());
            state.lock().unwrap().recursive = false;
            start_scan(
                &window,
                path.parent()
                    .unwrap_or(std::path::Path::new("."))
                    .to_owned(),
                state.clone(),
            );
        } else {
            state.lock().unwrap().recursive = true;
            start_scan(&window, path, state.clone());
        }
    } else {
        let weak = window.as_weak();
        slint::Timer::single_shot(std::time::Duration::ZERO, move || {
            if let Some(window) = weak.upgrade() {
                window.invoke_choose_folder();
            }
        });
    }
    let result = window.run();
    if let Some(player) = &player {
        player.stop();
    }
    result
}

fn start_scan(
    window: &AppWindow,
    directory: PathBuf,
    state: std::sync::Arc<std::sync::Mutex<State>>,
) {
    let directory = std::fs::canonicalize(&directory).unwrap_or(directory);
    let new_folder = window.get_folder().as_str() != directory.to_string_lossy();
    if new_folder {
        window.invoke_close_photo();
    }
    window.set_scan_completed(0);
    window.set_scan_total(-1);
    window.set_busy(true);
    window.set_folder(directory.to_string_lossy().as_ref().into());
    window.set_status("Scanning…".into());
    let (id, recursive, metadata_epoch) = {
        let mut s = state.lock().unwrap();
        s.scan_id = s.scan_id.wrapping_add(1);
        s.scanning = true;
        (s.scan_id, s.recursive, s.metadata_epoch)
    };
    let weak = window.as_weak();
    std::thread::spawn(move || {
        let progress_window = weak.clone();
        let progress_state = state.clone();
        let result =
            media::scan_depth_with_progress(&directory, recursive, move |completed, total| {
                let state = progress_state.clone();
                let _ = progress_window.upgrade_in_event_loop(move |window| {
                    let s = state.lock().unwrap();
                    if s.scan_id != id || !s.scanning {
                        return;
                    }
                    window.set_scan_completed(completed.min(i32::MAX as usize) as i32);
                    window.set_scan_total(total.map_or(-1, |n| n.min(i32::MAX as usize) as i32));
                });
            });
        let _ = weak.upgrade_in_event_loop(move |window| {
            if state.lock().unwrap().scan_id != id {
                return;
            }
            state.lock().unwrap().scanning = false;
            window.set_busy(false);
            match result {
                Ok(mut report) => {
                    for warning in &report.warnings {
                        eprintln!("{warning}");
                    }
                    let mut state = state.lock().unwrap();
                    // A scan started before a save may contain the old rating.
                    for entry in &mut report.images {
                        if let Some(&(epoch, rating)) = state.metadata_writes.get(&entry.path)
                            && epoch > metadata_epoch
                        {
                            entry.metadata["Rating"] = serde_json::json!(rating);
                            entry.metadata["XMP-xmp:Rating"] = serde_json::json!(rating);
                        }
                    }
                    state
                        .metadata_writes
                        .retain(|_, (epoch, _)| *epoch > metadata_epoch);
                    state.all = report.images;
                    state.edits = state
                        .all
                        .iter()
                        .map(|e| {
                            (
                                e.path.clone(),
                                catalog::Edit::load(e, state.settings.write_metadata),
                            )
                        })
                        .collect();
                    state.pairs = catalog::pairs(&state.all);
                    state.failed_thumbs.clear();
                    state.thumbs.clear();
                    features::project(&window, &mut state);
                    let pending = state.pending_open.take();
                    let open = pending.as_ref().and_then(|p| {
                        state.entries.iter().position(|e| &e.path == p).or_else(|| {
                            state.pairs.get(p).and_then(|other| {
                                state.entries.iter().position(|e| &e.path == other)
                            })
                        })
                    });
                    let missing = state
                        .active
                        .as_ref()
                        .is_some_and(|p| !state.all.iter().any(|e| &e.path == p));
                    let differs = open
                        .zip(pending.as_ref())
                        .is_some_and(|(i, p)| state.entries[i].path != *p);
                    drop(state);
                    if let Some(index) = open {
                        window.invoke_open_photo(index as i32);
                        if differs {
                            window.invoke_action("pair".into());
                        }
                    } else if missing {
                        window.invoke_close_photo();
                    } else if !window.get_viewer_open() {
                        window.invoke_focus_library();
                    }
                }
                Err(error) => window.set_status(format!("Could not scan folder: {error}").into()),
            }
        });
    });
}

fn show_rows(window: &AppWindow, state: &mut State) {
    features::models(window, state);
}

fn select(
    window: &AppWindow,
    state: &std::sync::Mutex<State>,
    viewer: &viewer::Viewer,
    player: Option<&video::Player>,
    index: i32,
) {
    let mut state = state.lock().unwrap();
    if index < 0 || index as usize >= state.entries.len() {
        return;
    }
    let index = index as usize;
    let backwards = state.selected.is_some_and(|previous| index < previous);
    state.selected = Some(index);
    window.set_library_selection(index as i32);
    let entry = state.entries[index].clone();
    state.active = Some(entry.path.clone());
    features::selection(window, &mut state, &entry);
    viewer.cancel();
    if let Some(player) = player {
        player.stop();
    }
    window.set_is_video(entry.video);
    window.set_viewer_open(true);
    window.set_photo(Default::default());
    window.set_photo_width(0.0);
    window.set_photo_height(0.0);
    window.set_photo_original_width(0.0);
    window.set_photo_full(false);
    window.set_photo_title(
        entry
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref()
            .into(),
    );
    window.set_photo_position(format!("{} / {}", index + 1, state.entries.len()).into());
    window.set_can_previous(index > 0);
    window.set_can_next(index + 1 < state.entries.len());
    window.set_photo_revision(window.get_photo_revision().wrapping_add(1));
    // Both pages persist: focus cannot be cleared by destroying the outgoing page.
    window.invoke_focus_viewer();
    if entry.video {
        if let Some(player) = player {
            player.open(window, &entry.path);
        }
        return;
    }
    window.set_video_texture(Default::default());
    viewer.open(
        window,
        &entry,
        viewer::neighbors(&state.entries, index, backwards),
        window.get_full_quality(),
    );
}
