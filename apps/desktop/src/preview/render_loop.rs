use super::scene::PreviewSize;
use super::*;

pub(crate) fn run_preview_loop(
    app: AppHandle,
    window: Window,
    mut renderer: PreviewRenderer,
    running: Arc<AtomicBool>,
    wake: PreviewWake,
) {
    let mut stats = PreviewFrameStats::new();
    let mut cached_scene: Option<PreviewScene> = None;
    let mut last_render_key: Option<PreviewRenderKey> = None;
    let mut reported_render_error = false;
    let mut wake_generation = wake.generation();
    while running.load(Ordering::Acquire) {
        let size = match window_size(&window) {
            Ok(size) => size,
            Err(_) => break,
        };

        let state = app.state::<crate::desktop_state::DesktopState>();
        match state.preview_scene_revision() {
            Some(revision)
                if cached_scene
                    .as_ref()
                    .is_none_or(|scene| scene.revision != revision) =>
            {
                cached_scene = match state.preview_scene() {
                    Ok(scene) => scene,
                    Err(error) => {
                        state.set_render_error_if_changed(error);
                        reported_render_error = true;
                        renderer.render(size, None, None);
                        wake_generation = wake.wait(wake_generation, &running);
                        continue;
                    }
                };
            }
            Some(_) => {}
            None => cached_scene = None,
        }

        let clock = match state.active_preview_render_identity() {
            Ok(clock) => Some(clock),
            Err(
                SequenceRenderError::NoSequenceRenderSession
                | SequenceRenderError::ClockUnavailable { .. },
            ) => None,
            Err(SequenceRenderError::Render(_)) => None,
        };
        let render_key = PreviewRenderKey::new(size, cached_scene.as_ref(), clock.as_ref());
        if last_render_key.as_ref() != Some(&render_key) {
            let frame = match state.render_current_sequence_frame() {
                Ok(rendered) => {
                    if reported_render_error {
                        state.clear_render_error_if_set();
                        reported_render_error = false;
                    }
                    Some(rendered.frame)
                }
                Err(
                    SequenceRenderError::NoSequenceRenderSession
                    | SequenceRenderError::ClockUnavailable { .. },
                ) => None,
                Err(SequenceRenderError::Render(error)) => {
                    state.set_render_error_if_changed(format!("Render failed: {error:?}"));
                    reported_render_error = true;
                    None
                }
            };
            renderer.render(size, cached_scene.as_ref(), frame.as_ref());
            last_render_key = Some(render_key);
            if let Some(fps) = stats.record_frame() {
                let _ = window.set_title(&format!("Dawn Preview - {fps:.0} FPS"));
            }
        }
        match preview_sleep_duration(clock.as_ref()) {
            Some(duration) => std::thread::sleep(duration),
            None => wake_generation = wake.wait(wake_generation, &running),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewRenderKey {
    size: PreviewSize,
    scene_revision: Option<u64>,
    clock: Option<PreviewClockKey>,
}

impl PreviewRenderKey {
    fn new(
        size: PreviewSize,
        scene: Option<&PreviewScene>,
        clock: Option<&AudioClockRenderIdentity>,
    ) -> Self {
        Self {
            size,
            scene_revision: scene.map(|scene| scene.revision),
            clock: clock.map(PreviewClockKey::new),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewClockKey {
    session_generation: u64,
    audio_generation: u32,
    audio_state: AudioTransportState,
    frame_rate: u32,
    frame_count: u32,
    frame_index: u32,
    paused_position_bits: Option<u32>,
}

impl PreviewClockKey {
    fn new(clock: &AudioClockRenderIdentity) -> Self {
        let paused_position_bits = if matches!(clock.audio_state, AudioTransportState::Playing) {
            None
        } else {
            Some(clock.position_seconds.to_bits())
        };
        Self {
            session_generation: clock.session_generation,
            audio_generation: clock.audio_generation,
            audio_state: clock.audio_state.clone(),
            frame_rate: clock.frame_rate,
            frame_count: clock.frame_count,
            frame_index: clock.frame_index,
            paused_position_bits,
        }
    }
}

fn preview_sleep_duration(clock: Option<&AudioClockRenderIdentity>) -> Option<Duration> {
    const MAX_PLAYING_SLEEP: Duration = Duration::from_millis(100);
    const MIN_PLAYING_SLEEP: Duration = Duration::from_millis(1);
    let clock = clock?;
    if !matches!(clock.audio_state, AudioTransportState::Playing) || clock.frame_rate == 0 {
        return None;
    }
    let next_frame_seconds = (clock.frame_index.saturating_add(1)) as f32 / clock.frame_rate as f32;
    let delay_seconds = (next_frame_seconds - clock.position_seconds).max(0.0);
    let delay = Duration::from_secs_f32(delay_seconds);
    Some(delay.clamp(MIN_PLAYING_SLEEP, MAX_PLAYING_SLEEP))
}

struct PreviewFrameStats {
    window_started: Instant,
    frame_count: u32,
}

impl PreviewFrameStats {
    fn new() -> Self {
        Self {
            window_started: Instant::now(),
            frame_count: 0,
        }
    }

    fn record_frame(&mut self) -> Option<f32> {
        self.frame_count = self.frame_count.saturating_add(1);
        let elapsed = self.window_started.elapsed();
        if elapsed < Duration::from_secs(1) {
            return None;
        }
        let fps = self.frame_count as f32 / elapsed.as_secs_f32();
        self.window_started = Instant::now();
        self.frame_count = 0;
        Some(fps)
    }
}

pub(crate) fn window_size(window: &Window) -> Result<PreviewSize, String> {
    let size = window.inner_size().map_err(|error| error.to_string())?;
    Ok(PreviewSize {
        width: size.width.max(1),
        height: size.height.max(1),
    })
}

pub(crate) fn empty_instance_buffer(device: &wgpu::Device, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: 16,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
