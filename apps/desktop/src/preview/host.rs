use std::io::BufRead;
use std::sync::Arc;
use std::time::Instant;

use donder_preview::{
    PreviewClockSnapshot, PreviewInstance, PreviewPlayback, PreviewPlaybackState,
    PreviewRenderOutcome, PreviewRenderer, PreviewScene, PreviewSize, PreviewStyle,
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use super::protocol::{PreviewCommand, PreviewEvent, PreviewStartup, PreviewWindowState};

enum HostEvent {
    Command(PreviewCommand),
    InputError(String),
    InputClosed,
}

pub(crate) fn startup_from_arguments() -> Result<Option<PreviewStartup>, String> {
    let mut arguments = std::env::args_os().skip(1);
    let Some(mode) = arguments.next() else {
        return Ok(None);
    };
    if mode != super::protocol::PREVIEW_HOST_ARGUMENT {
        return Ok(None);
    }
    let startup = arguments
        .next()
        .ok_or_else(|| "Preview host startup data is missing.".to_string())?;
    if arguments.next().is_some() {
        return Err("Preview host received unexpected arguments.".to_string());
    }
    serde_json::from_str(
        startup
            .to_str()
            .ok_or_else(|| "Preview host startup data is not UTF-8.".to_string())?,
    )
    .map(Some)
    .map_err(|error| format!("Cannot decode Preview host startup data: {error}"))
}

pub(crate) fn run(startup: PreviewStartup) -> Result<(), String> {
    let event_loop = EventLoop::<HostEvent>::with_user_event()
        .build()
        .map_err(|error| format!("Cannot create Preview event loop: {error}"))?;
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let event = match line {
                Ok(line) => match serde_json::from_str(&line) {
                    Ok(command) => HostEvent::Command(command),
                    Err(error) => {
                        HostEvent::InputError(format!("Cannot decode Preview command: {error}"))
                    }
                },
                Err(error) => {
                    HostEvent::InputError(format!("Cannot read Preview command: {error}"))
                }
            };
            if proxy.send_event(event).is_err() {
                return;
            }
        }
        let _ = proxy.send_event(HostEvent::InputClosed);
    });
    let mut application = PreviewHostApplication::new(startup)?;
    event_loop
        .run_app(&mut application)
        .map_err(|error| format!("Preview event loop failed: {error}"))
}

struct PreviewHostApplication {
    startup: PreviewStartup,
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    renderer: Option<PreviewRenderer>,
    scene: PreviewScene,
    playback: PreviewPlayback,
    style: PreviewStyle,
    next_wake: Option<Instant>,
    fps_interval_started_at: Instant,
    presented_frames: u32,
    closed_reported: bool,
}

impl PreviewHostApplication {
    fn new(startup: PreviewStartup) -> Result<Self, String> {
        let style = style(startup.appearance)?;
        Ok(Self {
            startup,
            window: None,
            surface: None,
            renderer: None,
            scene: PreviewScene::new(0, Vec::new()),
            playback: PreviewPlayback::new(style.unlit_color()),
            style,
            next_wake: None,
            fps_interval_started_at: Instant::now(),
            presented_frames: 0,
            closed_reported: false,
        })
    }

    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let appearance = self.startup.appearance;
        let mut attributes = Window::default_attributes()
            .with_title("Donder Preview — FPS: --")
            .with_inner_size(LogicalSize::new(
                appearance.window_width,
                appearance.window_height,
            ))
            .with_min_inner_size(LogicalSize::new(
                appearance.window_min_width,
                appearance.window_min_height,
            ));
        if let Some(saved) = self
            .startup
            .window
            .filter(|saved| window_state_is_visible(event_loop, *saved))
        {
            attributes = attributes
                .with_inner_size(PhysicalSize::new(saved.width, saved.height))
                .with_position(PhysicalPosition::new(saved.x, saved.y))
                .with_maximized(saved.maximized);
        }
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| format!("Cannot create Preview window: {error}"))?,
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(event_loop.owned_display_handle()),
        ));
        let surface = instance
            .create_surface(window.clone())
            .map_err(|error| format!("Cannot create Preview GPU surface: {error}"))?;
        let physical = window.inner_size();
        let renderer = tauri::async_runtime::block_on(PreviewRenderer::new(
            &instance,
            &surface,
            PreviewSize::nonzero(physical.width, physical.height),
        ))
        .map_err(|error| error.to_string())?;
        self.window = Some(window);
        self.surface = Some(surface);
        self.renderer = Some(renderer);
        emit(&PreviewEvent::Ready)
    }

    fn handle_command(&mut self, event_loop: &ActiveEventLoop, command: PreviewCommand) {
        let result = match command {
            PreviewCommand::ReplaceContent {
                revision,
                instances,
                sequence,
            } => self.replace_content(revision, instances, sequence),
            PreviewCommand::SetClock {
                generation,
                state,
                position_seconds,
                start_delay_seconds,
            } => {
                self.playback.set_clock(
                    PreviewClockSnapshot {
                        generation,
                        state: playback_state(state),
                        position_seconds,
                        start_delay_seconds,
                    },
                    Instant::now(),
                );
                Ok(())
            }
            PreviewCommand::SetAppearance { appearance } => self.set_appearance(appearance),
            PreviewCommand::Focus => {
                if let Some(window) = self.window.as_ref() {
                    window.focus_window();
                }
                Ok(())
            }
            PreviewCommand::Close => {
                self.close(event_loop);
                return;
            }
        };
        match result {
            Ok(()) => {
                self.next_wake = None;
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            Err(message) => self.report_error(message),
        }
    }

    fn replace_content(
        &mut self,
        revision: u64,
        instances: Vec<[f32; 4]>,
        sequence: Option<Vec<u8>>,
    ) -> Result<(), String> {
        self.playback
            .replace_content(sequence.as_deref(), instances.len())
            .map_err(|error| format!("Cannot load Preview content: {error:?}"))?;
        self.scene = PreviewScene::new(
            revision,
            instances
                .into_iter()
                .map(|center_radius| PreviewInstance { center_radius })
                .collect(),
        );
        Ok(())
    }

    fn set_appearance(&mut self, appearance: crate::dto::PreviewAppearance) -> Result<(), String> {
        self.style = style(appearance)?;
        self.startup.appearance = appearance;
        self.playback.set_unlit(self.style.unlit_color());
        if let Some(window) = self.window.as_ref() {
            window.set_min_inner_size(Some(LogicalSize::new(
                appearance.window_min_width,
                appearance.window_min_height,
            )));
        }
        Ok(())
    }

    fn render(&mut self) -> Result<(), String> {
        let now = Instant::now();
        self.playback
            .evaluate(now)
            .map_err(|error| format!("Cannot evaluate Preview frame: {error:?}"))?;
        let window = self
            .window
            .as_ref()
            .cloned()
            .ok_or_else(|| "Preview window is unavailable.".to_string())?;
        let surface = self
            .surface
            .as_ref()
            .ok_or_else(|| "Preview surface is unavailable.".to_string())?;
        let renderer = self
            .renderer
            .as_mut()
            .ok_or_else(|| "Preview renderer is unavailable.".to_string())?;
        let physical = window.inner_size();
        let outcome = renderer
            .render(
                surface,
                PreviewSize::nonzero(physical.width, physical.height),
                &self.scene,
                self.playback.colors(),
                self.style,
            )
            .map_err(|error| error.to_string())?;
        if outcome == PreviewRenderOutcome::Presented {
            self.record_presented_frame(&window, now);
        }
        self.next_wake = self.playback.next_deadline(now);
        Ok(())
    }

    fn record_presented_frame(&mut self, window: &Window, now: Instant) {
        self.presented_frames = self.presented_frames.saturating_add(1);
        let elapsed = now.duration_since(self.fps_interval_started_at);
        if elapsed.as_secs_f32() < 1.0 {
            return;
        }
        let fps = self.presented_frames as f32 / elapsed.as_secs_f32();
        window.set_title(&format!("Donder Preview — FPS: {fps:.1}"));
        self.fps_interval_started_at = now;
        self.presented_frames = 0;
    }

    fn report_geometry(&self, closed: bool) {
        let Some(window) = self.window.as_ref().and_then(|window| window_state(window)) else {
            return;
        };
        let event = if closed {
            PreviewEvent::Closed { window }
        } else {
            PreviewEvent::GeometryChanged { window }
        };
        let _ = emit(&event);
    }

    fn report_error(&self, message: String) {
        let _ = emit(&PreviewEvent::Error { message });
    }

    fn close(&mut self, event_loop: &ActiveEventLoop) {
        if !self.closed_reported {
            self.report_geometry(true);
            self.closed_reported = true;
        }
        event_loop.exit();
    }
}

impl ApplicationHandler<HostEvent> for PreviewHostApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(error) = self.initialize(event_loop) {
            self.report_error(error);
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: HostEvent) {
        match event {
            HostEvent::Command(command) => self.handle_command(event_loop, command),
            HostEvent::InputError(error) => self.report_error(error),
            HostEvent::InputClosed => self.close(event_loop),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => self.close(event_loop),
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.render() {
                    self.report_error(error);
                }
            }
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => self.report_geometry(false),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        match self.next_wake {
            Some(deadline) if deadline <= now => {
                self.next_wake = None;
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if !self.closed_reported {
            self.report_geometry(true);
            self.closed_reported = true;
        }
    }
}

fn style(appearance: crate::dto::PreviewAppearance) -> Result<PreviewStyle, String> {
    PreviewStyle {
        background_rgb: appearance.background_rgb,
        unlit_rgb: appearance.unlit_rgb,
        canvas_fill_ratio: appearance.canvas_fill_ratio,
        minimum_radius_pixels: appearance.minimum_radius_pixels,
    }
    .validate()
    .map_err(|error| format!("Preview appearance is invalid: {error:?}"))
}

fn playback_state(state: crate::dto::AudioTransportState) -> PreviewPlaybackState {
    match state {
        crate::dto::AudioTransportState::Playing => PreviewPlaybackState::Playing,
        crate::dto::AudioTransportState::Paused => PreviewPlaybackState::Paused,
        crate::dto::AudioTransportState::Stopped => PreviewPlaybackState::Stopped,
        crate::dto::AudioTransportState::Ended => PreviewPlaybackState::Ended,
        crate::dto::AudioTransportState::Unloaded | crate::dto::AudioTransportState::Error => {
            PreviewPlaybackState::Unavailable
        }
    }
}

fn window_state(window: &Window) -> Option<PreviewWindowState> {
    if window.is_minimized().unwrap_or(false) {
        return None;
    }
    let position = window.outer_position().ok()?;
    let size = window.inner_size();
    Some(PreviewWindowState {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        maximized: window.is_maximized(),
    })
}

fn window_state_is_visible(event_loop: &ActiveEventLoop, state: PreviewWindowState) -> bool {
    if state.width == 0 || state.height == 0 {
        return false;
    }
    event_loop.available_monitors().any(|monitor| {
        let position = monitor.position();
        let size = monitor.size();
        if state.width > size.width || state.height > size.height {
            return false;
        }
        let window_right = i64::from(state.x) + i64::from(state.width);
        let window_bottom = i64::from(state.y) + i64::from(state.height);
        let monitor_right = i64::from(position.x) + i64::from(size.width);
        let monitor_bottom = i64::from(position.y) + i64::from(size.height);
        i64::from(state.x) < monitor_right
            && window_right > i64::from(position.x)
            && i64::from(state.y) < monitor_bottom
            && window_bottom > i64::from(position.y)
    })
}

fn emit(event: &PreviewEvent) -> Result<(), String> {
    let stdout = std::io::stdout();
    super::protocol::write_message(&mut stdout.lock(), event)
}
