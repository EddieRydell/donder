use std::io::{BufRead, BufReader, BufWriter};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::dto::{AudioTransportSnapshot, AudioTransportState, PreviewAppearance};
use crate::persistence::{PersistedPreviewWindowState, PersistenceService};

mod geometry;
mod host;
mod protocol;

pub(crate) use geometry::{PreviewGeometry, point3_meters};
use protocol::{PreviewCommand, PreviewEvent, PreviewStartup, write_message};

type PreviewWriter = Arc<Mutex<BufWriter<ChildStdin>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PreviewContentIdentity {
    pub(crate) project_epoch: u32,
    pub(crate) project_revision: u64,
    pub(crate) sequence_generation: u64,
    pub(crate) project_loaded: bool,
}

pub(crate) struct PreviewContent {
    pub(crate) instances: Vec<[f32; 4]>,
    pub(crate) sequence: Option<Vec<u8>>,
}

struct PreviewProcess {
    id: u64,
    writer: PreviewWriter,
    active: Arc<AtomicBool>,
    preserve_open: Arc<AtomicBool>,
}

struct PreviewServiceState {
    appearance: Option<PreviewAppearance>,
    process: Option<PreviewProcess>,
    next_process_id: u64,
}

struct PreviewMonitor {
    app: AppHandle,
    id: u64,
    service: Arc<Mutex<PreviewServiceState>>,
    active: Arc<AtomicBool>,
    preserve_open: Arc<AtomicBool>,
    wake: PreviewWake,
}

pub(crate) struct PreviewWindowService {
    state: Arc<Mutex<PreviewServiceState>>,
    wake: PreviewWake,
}

impl PreviewWindowService {
    pub(crate) fn new(wake: PreviewWake) -> Self {
        Self {
            state: Arc::new(Mutex::new(PreviewServiceState {
                appearance: None,
                process: None,
                next_process_id: 1,
            })),
            wake,
        }
    }

    pub(crate) fn set_appearance(&self, appearance: PreviewAppearance) -> Result<(), String> {
        validate_appearance(appearance)?;
        let writer = {
            let mut state = lock(&self.state);
            state.appearance = Some(appearance);
            state.process.as_ref().and_then(|process| {
                process
                    .active
                    .load(Ordering::Acquire)
                    .then(|| process.writer.clone())
            })
        };
        if let Some(writer) = writer {
            send(&writer, &PreviewCommand::SetAppearance { appearance })?;
        }
        Ok(())
    }

    pub(crate) fn open_or_focus(
        &self,
        app: AppHandle,
        restore: PersistedPreviewWindowState,
    ) -> Result<(), String> {
        let (id, appearance, existing) = {
            let mut state = lock(&self.state);
            if let Some(process) = state
                .process
                .as_ref()
                .filter(|process| process.active.load(Ordering::Acquire))
            {
                (process.id, None, Some(process.writer.clone()))
            } else {
                state.process = None;
                let appearance = state
                    .appearance
                    .ok_or_else(|| "Preview appearance has not been initialized.".to_string())?;
                let id = state.next_process_id;
                state.next_process_id = state.next_process_id.saturating_add(1);
                (id, Some(appearance), None)
            }
        };
        if let Some(writer) = existing {
            return send(&writer, &PreviewCommand::Focus);
        }
        let appearance =
            appearance.ok_or_else(|| "Preview appearance is unavailable.".to_string())?;
        self.spawn(app, id, appearance, restore)
    }

    fn spawn(
        &self,
        app: AppHandle,
        id: u64,
        appearance: PreviewAppearance,
        restore: PersistedPreviewWindowState,
    ) -> Result<(), String> {
        let startup = PreviewStartup {
            appearance,
            window: restore.geometry.map(Into::into),
        };
        let startup = serde_json::to_string(&startup)
            .map_err(|error| format!("Cannot encode Preview startup data: {error}"))?;
        let executable = std::env::current_exe()
            .map_err(|error| format!("Cannot locate the Donder executable: {error}"))?;
        let mut child = Command::new(executable)
            .arg(protocol::PREVIEW_HOST_ARGUMENT)
            .arg(startup)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("Cannot launch the Preview process: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Preview process input is unavailable.".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Preview process output is unavailable.".to_string())?;
        let writer = Arc::new(Mutex::new(BufWriter::new(stdin)));
        let active = Arc::new(AtomicBool::new(true));
        let preserve_open = Arc::new(AtomicBool::new(false));
        {
            let mut state = lock(&self.state);
            state.process = Some(PreviewProcess {
                id,
                writer: writer.clone(),
                active: active.clone(),
                preserve_open: preserve_open.clone(),
            });
        }

        let monitor = PreviewMonitor {
            app: app.clone(),
            id,
            service: self.state.clone(),
            active: active.clone(),
            preserve_open: preserve_open.clone(),
            wake: self.wake.clone(),
        };
        std::thread::spawn(move || {
            monitor_process(monitor, child, stdout);
        });

        let sync_wake = self.wake.clone();
        std::thread::spawn(move || run_sync_loop(app, writer, active, sync_wake));
        Ok(())
    }

    pub(crate) fn close(&self, persistence: &PersistenceService) -> Result<(), String> {
        persistence.record_preview_window(PersistedPreviewWindowState {
            open: false,
            geometry: persistence.preview_window().geometry,
        })?;
        self.close_process(false)
    }

    pub(crate) fn close_for_main_shutdown(
        &self,
        persistence: &PersistenceService,
    ) -> Result<(), String> {
        let preview_open = lock(&self.state)
            .process
            .as_ref()
            .is_some_and(|process| process.active.load(Ordering::Acquire));
        if !preview_open {
            return Ok(());
        }
        persistence.record_preview_window(PersistedPreviewWindowState {
            open: true,
            geometry: persistence.preview_window().geometry,
        })?;
        self.close_process(true)
    }

    fn close_process(&self, preserve_open: bool) -> Result<(), String> {
        let process = {
            let state = lock(&self.state);
            state.process.as_ref().map(|process| {
                process
                    .preserve_open
                    .store(preserve_open, Ordering::Release);
                process.active.store(false, Ordering::Release);
                process.writer.clone()
            })
        };
        self.wake.notify();
        if let Some(writer) = process {
            send(&writer, &PreviewCommand::Close)?;
        }
        Ok(())
    }
}

fn monitor_process(
    monitor: PreviewMonitor,
    mut child: std::process::Child,
    stdout: std::process::ChildStdout,
) {
    let mut reported_closed = false;
    let mut reported_error = false;
    for line in BufReader::new(stdout).lines() {
        let event = match line {
            Ok(line) => serde_json::from_str::<PreviewEvent>(&line)
                .map_err(|error| format!("Cannot decode Preview event: {error}")),
            Err(error) => Err(format!("Cannot read Preview event: {error}")),
        };
        match event {
            Ok(PreviewEvent::Ready) => {
                let state = monitor.app.state::<crate::desktop_state::DesktopState>();
                state.update_snapshot(|snapshot| {
                    snapshot.preview_open = true;
                    snapshot.preview_error = None;
                });
            }
            Ok(PreviewEvent::GeometryChanged { window }) => {
                let state = monitor.app.state::<crate::desktop_state::DesktopState>();
                let _ = state
                    .persistence()
                    .record_preview_window(PersistedPreviewWindowState {
                        open: true,
                        geometry: Some(window.into()),
                    });
            }
            Ok(PreviewEvent::Closed { window }) => {
                reported_closed = true;
                let state = monitor.app.state::<crate::desktop_state::DesktopState>();
                let keep_open = monitor.preserve_open.load(Ordering::Acquire);
                let _ = state
                    .persistence()
                    .record_preview_window(PersistedPreviewWindowState {
                        open: keep_open,
                        geometry: Some(window.into()),
                    });
                if !keep_open {
                    let snapshot = state.update_snapshot(|snapshot| {
                        snapshot.preview_open = false;
                    });
                    let _ = monitor
                        .app
                        .emit("preview_window_changed", snapshot.preview_open);
                }
            }
            Ok(PreviewEvent::Error { message }) | Err(message) => {
                reported_error = true;
                let state = monitor.app.state::<crate::desktop_state::DesktopState>();
                state.update_snapshot(|snapshot| {
                    snapshot.preview_error = Some(format!("Preview failed: {message}"));
                });
            }
        }
    }
    let exit = child.wait();
    monitor.active.store(false, Ordering::Release);
    monitor.wake.notify();
    {
        let mut state = lock(&monitor.service);
        if state
            .process
            .as_ref()
            .is_some_and(|process| process.id == monitor.id)
        {
            state.process = None;
        }
    }
    if !reported_closed && !monitor.preserve_open.load(Ordering::Acquire) {
        let state = monitor.app.state::<crate::desktop_state::DesktopState>();
        let _ = state
            .persistence()
            .record_preview_window(PersistedPreviewWindowState {
                open: false,
                geometry: state.persistence().preview_window().geometry,
            });
        let message = match exit {
            Ok(status) if status.success() || reported_error => None,
            Ok(status) => Some(format!("Preview process exited with {status}.")),
            Err(error) => Some(format!("Cannot wait for Preview process: {error}")),
        };
        let snapshot = state.update_snapshot(|snapshot| {
            snapshot.preview_open = false;
            if let Some(message) = message {
                snapshot.preview_error = Some(message);
            }
        });
        let _ = monitor
            .app
            .emit("preview_window_changed", snapshot.preview_open);
    }
}

fn run_sync_loop(
    app: AppHandle,
    writer: PreviewWriter,
    active: Arc<AtomicBool>,
    wake: PreviewWake,
) {
    let mut wake_generation = wake.generation();
    let mut content_identity = None;
    let mut content_revision = 0u64;
    let mut clock_key = None;
    while active.load(Ordering::Acquire) {
        let state = app.state::<crate::desktop_state::DesktopState>();
        let identity = state.preview_content_identity();
        if content_identity != Some(identity) {
            match state.preview_content() {
                Ok(content) => {
                    content_revision = content_revision.saturating_add(1);
                    if send(
                        &writer,
                        &PreviewCommand::ReplaceContent {
                            revision: content_revision,
                            instances: content.instances,
                            sequence: content.sequence,
                        },
                    )
                    .is_err()
                    {
                        active.store(false, Ordering::Release);
                        break;
                    }
                    content_identity = Some(identity);
                }
                Err(error) => {
                    state.update_snapshot(|snapshot| {
                        snapshot.preview_error = Some(format!("Preview failed: {error}"));
                    });
                }
            }
        }

        let clock = state.audio_snapshot();
        let next_clock_key = PreviewClockKey::from(&clock);
        let playing = matches!(clock.state, AudioTransportState::Playing);
        if (playing || clock_key.as_ref() != Some(&next_clock_key))
            && send_clock(&writer, clock).is_err()
        {
            active.store(false, Ordering::Release);
            break;
        }
        clock_key = Some(next_clock_key);
        if playing {
            wake_generation =
                wake.wait_timeout(wake_generation, &active, Duration::from_millis(50));
        } else {
            wake_generation = wake.wait(wake_generation, &active);
        }
    }
}

fn send_clock(writer: &PreviewWriter, clock: AudioTransportSnapshot) -> Result<(), String> {
    send(
        writer,
        &PreviewCommand::SetClock {
            generation: clock.generation,
            state: clock.state,
            position_seconds: clock.position_seconds,
            start_delay_seconds: clock.start_delay_seconds,
        },
    )
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewClockKey {
    generation: u32,
    state: AudioTransportState,
    position_bits: u32,
}

impl From<&AudioTransportSnapshot> for PreviewClockKey {
    fn from(value: &AudioTransportSnapshot) -> Self {
        Self {
            generation: value.generation,
            state: value.state.clone(),
            position_bits: value.position_seconds.to_bits(),
        }
    }
}

fn validate_appearance(appearance: PreviewAppearance) -> Result<(), String> {
    if appearance.window_width == 0
        || appearance.window_height == 0
        || appearance.window_min_width == 0
        || appearance.window_min_height == 0
    {
        return Err("Preview window dimensions must be positive.".to_string());
    }
    donder_preview::PreviewStyle {
        background_rgb: appearance.background_rgb,
        unlit_rgb: appearance.unlit_rgb,
        canvas_fill_ratio: appearance.canvas_fill_ratio,
        minimum_radius_pixels: appearance.minimum_radius_pixels,
    }
    .validate()
    .map(|_| ())
    .map_err(|error| format!("Preview appearance is invalid: {error:?}"))
}

fn send(writer: &PreviewWriter, command: &PreviewCommand) -> Result<(), String> {
    let mut writer = lock(writer);
    write_message(&mut *writer, command)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone)]
pub(crate) struct PreviewWake {
    state: Arc<(Mutex<u64>, Condvar)>,
}

impl PreviewWake {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new((Mutex::new(0), Condvar::new())),
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        let (generation, _) = &*self.state;
        *lock(generation)
    }

    pub(crate) fn notify(&self) {
        let (generation, wake) = &*self.state;
        let mut generation = lock(generation);
        *generation = generation.saturating_add(1);
        wake.notify_all();
    }

    pub(crate) fn wait(&self, observed: u64, running: &AtomicBool) -> u64 {
        let (generation, wake) = &*self.state;
        let generation = lock(generation);
        let generation = wake
            .wait_while(generation, |generation| {
                *generation == observed && running.load(Ordering::Acquire)
            })
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *generation
    }

    pub(crate) fn wait_timeout(
        &self,
        observed: u64,
        running: &AtomicBool,
        timeout: Duration,
    ) -> u64 {
        let (generation, wake) = &*self.state;
        let generation = lock(generation);
        let (generation, _) = wake
            .wait_timeout_while(generation, timeout, |generation| {
                *generation == observed && running.load(Ordering::Acquire)
            })
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *generation
    }
}

pub(crate) use host::{run as run_host, startup_from_arguments};
