mod render;

use super::AppWindow;
use libmpv2::{Format, Mpv};
use libmpv2_sys as sys;
use slint::ComponentHandle;
use std::{
    ffi::CStr,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
};

enum Command {
    Load(String, i64),
    Rotate(i64),
    Stop,
    Pause(bool),
    Seek(f64),
    FrameStep(bool),
    Volume(f64),
    Mute(bool),
    Shutdown,
}
/// UI commands never call blocking libmpv functions on Slint's GL thread.
pub struct Player {
    tx: mpsc::Sender<(u64, Command)>,
    worker: thread::Thread,
    generation: Arc<AtomicU64>,
}
impl Player {
    pub fn new(window: &AppWindow) -> Result<Self, String> {
        let core = Arc::new(
            Mpv::with_initializer(|init| {
                for (key, value) in [
                    ("vo", "libmpv"),
                    ("video-timing-offset", "0"),
                    ("terminal", "no"),
                    ("config", "no"),
                    ("load-scripts", "no"),
                    ("input-default-bindings", "no"),
                    ("input-vo-keyboard", "no"),
                    ("osc", "no"),
                    ("osd-level", "0"),
                    ("keep-open", "yes"),
                    ("hwdec", "auto-safe"),
                    ("audio-display", "no"),
                    ("ytdl", "no"),
                ] {
                    init.set_option(key, value)?;
                }
                Ok(())
            })
            .map_err(error_text)?,
        );
        let mut events = core
            .create_client(Some("viewer-events"))
            .map_err(error_text)?;
        for event in [
            sys::mpv_event_id_MPV_EVENT_START_FILE,
            sys::mpv_event_id_MPV_EVENT_FILE_LOADED,
            sys::mpv_event_id_MPV_EVENT_END_FILE,
        ] {
            events.enable_event(event).map_err(error_text)?;
        }
        render::register(window, core.clone())?;
        let (tx, rx) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let gen_worker = generation.clone();
        let weak = window.as_weak();
        let handle = thread::spawn(move || {
            let wake = thread::current();
            events.set_wakeup_callback(move || wake.unpark());
            let mut id = 0;
            let mut expected = None;
            let mut playing = None;
            loop {
                while let Ok((next, command)) = rx.try_recv() {
                    if matches!(command, Command::Shutdown) {
                        let _ = core.command("stop", &[]);
                        // Replace the owning callback before destroying the event client.
                        events.set_wakeup_callback(|| {});
                        return;
                    }
                    if next != gen_worker.load(Ordering::Acquire) {
                        continue;
                    }
                    let result = match command {
                        Command::Load(path, rotation) => {
                            let _ = events.unobserve_property(id);
                            id = next;
                            expected = None;
                            // Discard pending old observations before registering this generation.
                            unsafe {
                                while (*sys::mpv_wait_event(events.ctx.as_ptr(), 0.0)).event_id != 0
                                {
                                }
                            }
                            let result = core
                                .set_property("video-rotate", rotation)
                                .and_then(|()| core.command("loadfile", &[&path, "replace"]));
                            if result.is_ok() {
                                expected = core.get_property::<i64>("playlist/0/id").ok();
                                let _ = core.set_property("pause", false);
                                for (name, format) in [
                                    ("time-pos", Format::Double),
                                    ("duration", Format::Double),
                                    ("pause", Format::Flag),
                                    ("volume", Format::Double),
                                    ("mute", Format::Flag),
                                    ("eof-reached", Format::Flag),
                                ] {
                                    let _ = events.observe_property(name, format, id);
                                }
                            }
                            result
                        }
                        Command::Stop => {
                            expected = None;
                            let _ = events.unobserve_property(id);
                            id = next;
                            core.command("stop", &[])
                        }
                        Command::Pause(paused) => {
                            if !paused && core.get_property::<bool>("eof-reached").unwrap_or(false)
                            {
                                let _ = core.command("seek", &["0", "absolute+exact"]);
                            }
                            core.set_property("pause", paused)
                        }
                        Command::Seek(seconds) => {
                            core.command("seek", &[&seconds.to_string(), "absolute+exact"])
                        }
                        Command::FrameStep(forward) => {
                            if core.get_property::<bool>("pause").unwrap_or(false) {
                                core.command(
                                    if forward {
                                        "frame-step"
                                    } else {
                                        "frame-back-step"
                                    },
                                    &[],
                                )
                            } else {
                                Ok(())
                            }
                        }
                        Command::Rotate(degrees) => core.set_property("video-rotate", degrees),
                        Command::Volume(volume) => core.set_property("volume", volume),
                        Command::Mute(mute) => core.set_property("mute", mute),
                        Command::Shutdown => unreachable!(),
                    };
                    if let Err(error) = result {
                        publish(&weak, &gen_worker, next, Update::Error(error_text(error)));
                    }
                }
                // One dedicated client owns this event queue. Copy data before the next wait.
                loop {
                    let event = unsafe { &*sys::mpv_wait_event(events.ctx.as_ptr(), 0.0) };
                    if event.event_id == 0 {
                        break;
                    }
                    let update = unsafe {
                        match event.event_id {
                            sys::mpv_event_id_MPV_EVENT_START_FILE => {
                                playing = Some(
                                    (*event.data.cast::<sys::mpv_event_start_file>())
                                        .playlist_entry_id,
                                );
                                None
                            }
                            sys::mpv_event_id_MPV_EVENT_FILE_LOADED
                                if expected.is_some() && playing == expected =>
                            {
                                Some(Update::Loaded)
                            }
                            sys::mpv_event_id_MPV_EVENT_END_FILE => {
                                let end = &*event.data.cast::<sys::mpv_event_end_file>();
                                if Some(end.playlist_entry_id) == expected
                                    && end.reason
                                        == sys::mpv_end_file_reason_MPV_END_FILE_REASON_ERROR
                                {
                                    Some(Update::Error(
                                        CStr::from_ptr(sys::mpv_error_string(end.error))
                                            .to_string_lossy()
                                            .into_owned(),
                                    ))
                                } else {
                                    None
                                }
                            }
                            sys::mpv_event_id_MPV_EVENT_PROPERTY_CHANGE
                                if event.reply_userdata == id && expected.is_some() =>
                            {
                                let property = &*event.data.cast::<sys::mpv_event_property>();
                                if property.data.is_null() {
                                    None
                                } else {
                                    let name = CStr::from_ptr(property.name).to_bytes();
                                    match (name, property.format) {
                                        (b"time-pos", sys::mpv_format_MPV_FORMAT_DOUBLE) => {
                                            Some(Update::Position(*property.data.cast::<f64>()))
                                        }
                                        (b"duration", sys::mpv_format_MPV_FORMAT_DOUBLE) => {
                                            Some(Update::Duration(*property.data.cast::<f64>()))
                                        }
                                        (b"volume", sys::mpv_format_MPV_FORMAT_DOUBLE) => {
                                            Some(Update::Volume(*property.data.cast::<f64>()))
                                        }
                                        (b"pause", sys::mpv_format_MPV_FORMAT_FLAG) => {
                                            Some(Update::Pause(*property.data.cast::<i32>() != 0))
                                        }
                                        (b"mute", sys::mpv_format_MPV_FORMAT_FLAG) => {
                                            Some(Update::Mute(*property.data.cast::<i32>() != 0))
                                        }
                                        (b"eof-reached", sys::mpv_format_MPV_FORMAT_FLAG) => {
                                            Some(Update::Ended(*property.data.cast::<i32>() != 0))
                                        }
                                        _ => None,
                                    }
                                }
                            }
                            _ => None,
                        }
                    };
                    if let Some(update) = update {
                        publish(&weak, &gen_worker, id, update);
                    }
                }
                thread::park(); // Commands and mpv's event callback wake this thread.
            }
        });
        Ok(Self {
            tx,
            worker: handle.thread().clone(),
            generation,
        })
    }
    fn send(&self, command: Command) {
        let _ = self
            .tx
            .send((self.generation.load(Ordering::Acquire), command));
        self.worker.unpark();
    }
    pub fn open(&self, window: &AppWindow, path: &Path) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        window.set_video_texture(Default::default());
        window.set_video_position(0.0);
        window.set_video_duration(0.0);
        window.set_video_paused(false);
        window.set_video_ended(false);
        if !window.get_video_available() {
            window.set_video_loading(false);
            if window.get_video_error().is_empty() {
                window.set_video_error("OpenGL video renderer is unavailable".into());
            }
            self.send(Command::Stop);
            return;
        }
        window.set_video_error("".into());
        window.set_video_loading(true);
        // mpv's client API requires UTF-8 filenames (including on Windows).
        match path.to_str() {
            Some(path) if !path.contains('\0') => self.send(Command::Load(
                path.into(),
                i64::from(window.get_rotation().rem_euclid(4) * 90),
            )),
            _ => {
                window.set_video_loading(false);
                window.set_video_error(
                    "Video filename must be valid UTF-8 without NUL characters".into(),
                );
                self.send(Command::Stop);
            }
        }
    }
    pub fn rotate(&self, quarter_turns: i32) {
        self.send(Command::Rotate(i64::from(quarter_turns.rem_euclid(4) * 90)));
    }
    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.send(Command::Stop);
    }
    pub fn pause(&self, value: bool) {
        self.send(Command::Pause(value));
    }
    pub fn seek(&self, value: f32) {
        if value.is_finite() {
            self.send(Command::Seek(f64::from(value.max(0.0))));
        }
    }
    pub fn frame_step(&self, forward: bool) {
        self.send(Command::FrameStep(forward));
    }
    pub fn volume(&self, value: f32) {
        if value.is_finite() {
            self.send(Command::Volume(f64::from(value.clamp(0.0, 100.0))));
        }
    }
    pub fn mute(&self, value: bool) {
        self.send(Command::Mute(value));
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
    }
}
enum Update {
    Loaded,
    Position(f64),
    Duration(f64),
    Pause(bool),
    Volume(f64),
    Mute(bool),
    Ended(bool),
    Error(String),
}
fn publish(weak: &slint::Weak<AppWindow>, generation: &Arc<AtomicU64>, id: u64, update: Update) {
    let generation = generation.clone();
    let _ = weak.upgrade_in_event_loop(move |window| {
        if generation.load(Ordering::Acquire) != id
            || !window.get_viewer_open()
            || !window.get_is_video()
        {
            return;
        }
        match update {
            Update::Loaded => window.set_video_loading(false),
            Update::Position(value) if value.is_finite() => {
                window.set_video_position(value.max(0.0) as f32)
            }
            Update::Duration(value) if value.is_finite() => {
                window.set_video_duration(value.max(0.0) as f32)
            }
            Update::Pause(value) => window.set_video_paused(value),
            Update::Volume(value) => window.set_video_volume(value as f32),
            Update::Mute(value) => window.set_video_muted(value),
            Update::Ended(value) => window.set_video_ended(value),
            Update::Error(error) => {
                window.set_video_loading(false);
                window.set_video_error(format!("Could not play video: {error}").into());
            }
            _ => {}
        }
    });
}

fn error_text(error: libmpv2::Error) -> String {
    match error {
        libmpv2::Error::Raw(code) => unsafe {
            CStr::from_ptr(sys::mpv_error_string(code))
                .to_string_lossy()
                .into_owned()
        },
        other => other.to_string(),
    }
}
