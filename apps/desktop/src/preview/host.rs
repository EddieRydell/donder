use std::sync::Arc;
use std::time::{Duration, Instant};

use super::playback::{
    PreviewClockSnapshot, PreviewClockUpdate, PreviewPlayback, PreviewPlaybackState,
};
use super::renderer::PreviewRenderer;
use super::scene::{PreviewInstance, PreviewScene, PreviewSize, PreviewStyle};
use donder_runtime::PlaybackRate;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};

use super::protocol::{PreviewCommand, PreviewEvent, PreviewStartup, PreviewWindowState};

enum HostEvent {
    Command(PreviewCommand),
    InputError(String),
    InputClosed,
}

enum CommandEffect {
    Redraw,
    Unchanged,
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
    let mut builder = EventLoop::<HostEvent>::with_user_event();
    // The Preview is part of Donder, not a separate app: no Dock icon or menu bar of its own.
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_default_menu(false);
    }
    let event_loop = builder
        .build()
        .map_err(|error| format!("Cannot create Preview event loop: {error}"))?;
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        loop {
            let event = match super::protocol::read_command(&mut stdin) {
                Ok(Some(command)) => HostEvent::Command(command),
                Ok(None) => break,
                Err(message) => HostEvent::InputError(message),
            };
            // A malformed frame leaves the stream unreadable: report it, then close.
            let failed = matches!(event, HostEvent::InputError(_));
            if proxy.send_event(event).is_err() {
                return;
            }
            if failed {
                break;
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
    /// When presented frames reach the display; known once the window exists.
    display: Option<DisplayClock>,
    /// When to draw again after the surface had no image to draw into.
    retry_at: Option<Instant>,
    fps_interval_started_at: Instant,
    shown_frames: u32,
    closed_reported: bool,
    modifiers: ModifiersState,
}

impl PreviewHostApplication {
    fn new(startup: PreviewStartup) -> Result<Self, String> {
        let style = PreviewStyle::from_appearance(startup.appearance)?;
        Ok(Self {
            startup,
            window: None,
            surface: None,
            renderer: None,
            scene: PreviewScene::new(0, Vec::new()),
            playback: PreviewPlayback::new(style.unlit_color()),
            style,
            display: None,
            retry_at: None,
            fps_interval_started_at: Instant::now(),
            shown_frames: 0,
            closed_reported: false,
            modifiers: ModifiersState::empty(),
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
        self.display = Some(DisplayClock::new(refresh_period(&window)?));
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
                archive,
            } => self
                .replace_content(revision, instances, archive)
                .map(|()| CommandEffect::Redraw),
            PreviewCommand::RigEvents { events } => {
                self.playback.apply_rig_events(&events);
                Ok(CommandEffect::Redraw)
            }
            PreviewCommand::SetClock {
                generation,
                state,
                position_seconds,
                start_delay_seconds,
                playback_speed,
            } => PlaybackRate::try_from(playback_speed).map(|rate| {
                let update = self.playback.set_clock(
                    PreviewClockSnapshot {
                        generation,
                        state: playback_state(state),
                        position_seconds,
                        start_delay_seconds,
                        rate,
                    },
                    Instant::now(),
                );
                match update {
                    PreviewClockUpdate::Reanchored => CommandEffect::Redraw,
                    PreviewClockUpdate::Absorbed => CommandEffect::Unchanged,
                }
            }),
            PreviewCommand::SetAppearance { appearance } => self
                .set_appearance(appearance)
                .map(|()| CommandEffect::Redraw),
            PreviewCommand::Focus => {
                if let Some(window) = self.window.as_ref() {
                    window.focus_window();
                }
                Ok(CommandEffect::Unchanged)
            }
            PreviewCommand::Close => {
                self.close(event_loop);
                return;
            }
        };
        match result {
            Ok(CommandEffect::Redraw) => {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            Ok(CommandEffect::Unchanged) => {}
            Err(message) => self.report_error(message),
        }
    }

    fn replace_content(
        &mut self,
        revision: u64,
        instances: Vec<[f32; 4]>,
        archive: Option<Vec<u8>>,
    ) -> Result<(), String> {
        self.playback
            .replace_content(archive.as_deref(), instances.len())
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

    fn set_appearance(
        &mut self,
        appearance: donder_editor::dto::PreviewAppearance,
    ) -> Result<(), String> {
        self.style = PreviewStyle::from_appearance(appearance)?;
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

    /// Draw the frame the display shows next. While playing, a redraw follows every present and
    /// waiting for the surface image paces the loop to the display's refresh.
    fn render(&mut self) -> Result<(), String> {
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
        let display = self
            .display
            .as_mut()
            .ok_or_else(|| "Preview display clock is unavailable.".to_string())?;
        let physical = window.inner_size();
        let Some(output) = renderer
            .acquire(
                surface,
                PreviewSize::nonzero(physical.width, physical.height),
            )
            .map_err(|error| error.to_string())?
        else {
            display.reset();
            self.retry_at = Some(Instant::now() + display.refresh);
            return Ok(());
        };
        let acquired_at = Instant::now();
        let shown_at = if self.playback.is_playing() {
            display.advance(&window, acquired_at)?
        } else {
            display.reset();
            acquired_at
        };
        let changed = self
            .playback
            .evaluate(shown_at)
            .map_err(|error| format!("Cannot evaluate Preview frame: {error:?}"))?;
        // The minimum radius is authored in logical pixels; the surface is in physical pixels.
        let style = PreviewStyle {
            minimum_radius_pixels: self.style.minimum_radius_pixels * window.scale_factor() as f32,
            ..self.style
        };
        renderer
            .present(output, &self.scene, self.playback.colors(), style)
            .map_err(|error| error.to_string())?;
        self.record_presented_frame(&window, acquired_at, changed);
        Ok(())
    }

    /// The title counts the presents that showed a new frame.
    fn record_presented_frame(&mut self, window: &Window, now: Instant, changed: bool) {
        if changed {
            self.shown_frames = self.shown_frames.saturating_add(1);
        }
        let elapsed = now.duration_since(self.fps_interval_started_at);
        if elapsed.as_secs_f32() < 1.0 {
            return;
        }
        let fps = self.shown_frames as f32 / elapsed.as_secs_f32();
        window.set_title(&format!("Donder Preview — FPS: {fps:.1}"));
        self.fps_interval_started_at = now;
        self.shown_frames = 0;
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

    /// Keys go to Donder for performing. Without a menu bar of its own, the macOS Preview also
    /// handles the standard window shortcuts: Command-W closes it and Command-Q asks Donder to quit.
    fn handle_key(&mut self, event_loop: &ActiveEventLoop, event: &KeyEvent) {
        if event.repeat {
            return;
        }
        if let PhysicalKey::Code(code) = event.physical_key {
            // winit names its key codes as `KeyboardEvent.code` does.
            let _ = emit(&PreviewEvent::Key {
                code: format!("{code:?}"),
                pressed: event.state == ElementState::Pressed,
            });
        }
        if !cfg!(target_os = "macos") || event.state != ElementState::Pressed {
            return;
        }
        if !self.modifiers.super_key() {
            return;
        }
        match event.logical_key.as_ref() {
            Key::Character(key) if key.eq_ignore_ascii_case("w") => self.close(event_loop),
            Key::Character(key) if key.eq_ignore_ascii_case("q") => {
                let _ = emit(&PreviewEvent::QuitRequested);
            }
            _ => {}
        }
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
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } => self.handle_key(event_loop, &event),
            WindowEvent::Focused(false) => {
                let _ = emit(&PreviewEvent::KeysReleased);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.playback.is_playing() {
            self.retry_at = None;
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        if let Some(retry_at) = self.retry_at
            && Instant::now() < retry_at
        {
            event_loop.set_control_flow(ControlFlow::WaitUntil(retry_at));
            return;
        }
        self.retry_at = None;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if !self.closed_reported {
            self.report_geometry(true);
            self.closed_reported = true;
        }
    }
}

/// When presented frames reach the display. While presents keep up, they are exactly one refresh
/// apart, so playback samples evenly spaced show times and shows every show frame that lasts at
/// least a refresh. A late present, or a window moved to another display, re-anchors the grid on
/// the measured time.
struct DisplayClock {
    refresh: Duration,
    next: Option<Instant>,
}

impl DisplayClock {
    fn new(refresh: Duration) -> Self {
        Self {
            refresh,
            next: None,
        }
    }

    /// The display time of the frame whose surface image was acquired at `acquired_at`: one
    /// refresh later, when the frame presented before it leaves the display.
    fn advance(&mut self, window: &Window, acquired_at: Instant) -> Result<Instant, String> {
        let measured = acquired_at + self.refresh;
        let on_grid = self.next.filter(|expected| {
            expected
                .max(&measured)
                .duration_since(*expected.min(&measured))
                <= self.refresh / 2
        });
        let shown_at = match on_grid {
            Some(expected) => expected,
            None => {
                self.refresh = refresh_period(window)?;
                acquired_at + self.refresh
            }
        };
        self.next = Some(shown_at + self.refresh);
        Ok(shown_at)
    }

    fn reset(&mut self) {
        self.next = None;
    }
}

fn refresh_period(window: &Window) -> Result<Duration, String> {
    let millihertz = window
        .current_monitor()
        .and_then(|monitor| monitor.refresh_rate_millihertz())
        .filter(|millihertz| *millihertz > 0)
        .ok_or_else(|| "Cannot read the Preview display's refresh rate.".to_string())?;
    Ok(Duration::from_secs(1000) / millihertz)
}

fn playback_state(state: donder_editor::dto::AudioTransportState) -> PreviewPlaybackState {
    match state {
        donder_editor::dto::AudioTransportState::Playing => PreviewPlaybackState::Playing,
        donder_editor::dto::AudioTransportState::Paused => PreviewPlaybackState::Paused,
        donder_editor::dto::AudioTransportState::Stopped => PreviewPlaybackState::Stopped,
        donder_editor::dto::AudioTransportState::Ended => PreviewPlaybackState::Ended,
        donder_editor::dto::AudioTransportState::Unloaded
        | donder_editor::dto::AudioTransportState::Error => PreviewPlaybackState::Unavailable,
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
    super::protocol::write_event(&mut stdout.lock(), event)
}
