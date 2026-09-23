use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use donder_language::model::DonderProject;
use glam::Vec2;
use tauri::async_runtime::block_on;
use tauri::window::WindowBuilder;
use tauri::{AppHandle, Emitter, Manager, Window};
use wgpu::util::DeviceExt;

use crate::dto::AudioTransportState;
use crate::rendering::AudioClockRenderIdentity;
use crate::rendering::SequenceRenderError;

mod geometry;

pub(crate) use geometry::point3_meters;

pub const PREVIEW_LABEL: &str = "preview";
const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

pub(crate) struct PreviewWindowService {
    running: Mutex<Option<Arc<AtomicBool>>>,
    closing_for_main_shutdown: Arc<AtomicBool>,
    wake: PreviewWake,
}

impl PreviewWindowService {
    pub(crate) fn new(wake: PreviewWake) -> Self {
        Self {
            running: Mutex::new(None),
            closing_for_main_shutdown: Arc::new(AtomicBool::new(false)),
            wake,
        }
    }

    pub fn open_or_focus(
        &self,
        app: AppHandle,
        restore: crate::persistence::PersistedPreviewWindowState,
    ) -> Result<(), String> {
        if let Some(window) = app.get_window(PREVIEW_LABEL) {
            window.set_focus().map_err(|error| error.to_string())?;
            return Ok(());
        }
        self.closing_for_main_shutdown
            .store(false, Ordering::Release);

        let window = WindowBuilder::new(&app, PREVIEW_LABEL)
            .title("Donder Preview")
            .inner_size(960.0, 640.0)
            .min_inner_size(360.0, 240.0)
            .center()
            .build()
            .map_err(|error| error.to_string())?;
        if let Some(geometry) = restore.geometry.as_ref() {
            crate::persistence::apply_window_state(&window, geometry);
        }
        let renderer = PreviewRenderer::new(&window)?;
        let running = Arc::new(AtomicBool::new(true));

        {
            let mut current = self
                .running
                .lock()
                .map_err(|_| "Preview lifecycle lock is poisoned.".to_string())?;
            *current = Some(running.clone());
        }

        let close_flag = running.clone();
        let close_app = app.clone();
        let closing_for_main_shutdown = self.closing_for_main_shutdown.clone();
        let window_wake = self.wake.clone();
        window.on_window_event(move |event| match event {
            tauri::WindowEvent::CloseRequested { .. } => {
                close_flag.store(false, Ordering::Release);
                window_wake.notify();
                if closing_for_main_shutdown.load(Ordering::Acquire) {
                    return;
                }
                let state = close_app.state::<crate::desktop_state::DesktopState>();
                let geometry = close_app.get_window(PREVIEW_LABEL).and_then(|window| {
                    crate::persistence::read_window_state_or(
                        &window,
                        state.persistence().preview_window().geometry,
                    )
                });
                let _ = state.persistence().record_preview_window(
                    crate::persistence::PersistedPreviewWindowState {
                        open: false,
                        geometry,
                    },
                );
                let snapshot = state.update_snapshot(|snapshot| {
                    snapshot.preview_open = false;
                });
                let _ = close_app.emit("preview_window_changed", snapshot.preview_open);
            }
            tauri::WindowEvent::Destroyed => {
                close_flag.store(false, Ordering::Release);
                window_wake.notify();
            }
            tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                let state = close_app.state::<crate::desktop_state::DesktopState>();
                let geometry = close_app.get_window(PREVIEW_LABEL).and_then(|window| {
                    crate::persistence::read_window_state_or(
                        &window,
                        state.persistence().preview_window().geometry,
                    )
                });
                let _ = state.persistence().record_preview_window(
                    crate::persistence::PersistedPreviewWindowState {
                        open: true,
                        geometry,
                    },
                );
                if matches!(event, tauri::WindowEvent::Resized(_)) {
                    window_wake.notify();
                }
            }
            _ => {}
        });

        let render_wake = self.wake.clone();
        std::thread::spawn(move || {
            run_preview_loop(app, window, renderer, running, render_wake);
        });

        Ok(())
    }

    pub fn close(
        &self,
        app: &AppHandle,
        persistence: &crate::persistence::PersistenceService,
    ) -> Result<(), String> {
        let Some(window) = app.get_window(PREVIEW_LABEL) else {
            persistence.record_preview_window(crate::persistence::PersistedPreviewWindowState {
                open: false,
                geometry: persistence.preview_window().geometry,
            })?;
            return Ok(());
        };
        let geometry = crate::persistence::read_window_state_or(
            &window,
            persistence.preview_window().geometry,
        );
        persistence.record_preview_window(crate::persistence::PersistedPreviewWindowState {
            open: false,
            geometry,
        })?;
        window.close().map_err(|error| error.to_string())
    }

    pub fn close_for_main_shutdown(
        &self,
        app: &AppHandle,
        persistence: &crate::persistence::PersistenceService,
    ) -> Result<(), String> {
        let Some(window) = app.get_window(PREVIEW_LABEL) else {
            return Ok(());
        };
        let geometry = crate::persistence::read_window_state_or(
            &window,
            persistence.preview_window().geometry,
        );
        persistence.record_preview_window(crate::persistence::PersistedPreviewWindowState {
            open: true,
            geometry,
        })?;
        self.closing_for_main_shutdown
            .store(true, Ordering::Release);
        window.close().map_err(|error| error.to_string())
    }
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
        *generation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn notify(&self) {
        let (generation, wake) = &*self.state;
        let mut generation = generation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *generation = generation.saturating_add(1);
        wake.notify_all();
    }

    pub(crate) fn wait(&self, observed: u64, running: &AtomicBool) -> u64 {
        let (generation, wake) = &*self.state;
        let generation = generation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let generation = wake
            .wait_while(generation, |generation| {
                *generation == observed && running.load(Ordering::Acquire)
            })
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *generation
    }
}

mod render_loop;
mod renderer;
mod scene;

pub(crate) use render_loop::run_preview_loop;
pub(crate) use renderer::PreviewRenderer;
pub(crate) use scene::PreviewScene;

const PREVIEW_SHADER: &str = include_str!("preview.wgsl");
