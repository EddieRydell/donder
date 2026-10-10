use donder_sequence_api::{AppSettings, WorkspaceLayoutState};

pub(crate) fn sanitize_workspace_layout(state: WorkspaceLayoutState) -> WorkspaceLayoutState {
    WorkspaceLayoutState {
        sidebar_width_px: clamp_f32(state.sidebar_width_px, 220.0, 520.0),
        inspector_width_px: clamp_f32(state.inspector_width_px, 240.0, 560.0),
        sidebar_collapsed: state.sidebar_collapsed,
        inspector_collapsed: state.inspector_collapsed,
        active_sidebar_view: state.active_sidebar_view,
    }
}

fn clamp_f32(value: f32, min: f32, max: f32) -> f32 {
    if !value.is_finite() {
        return min;
    }
    value.clamp(min, max)
}

pub(crate) fn sanitize_app_settings(mut settings: AppSettings) -> AppSettings {
    if !settings.sequence_initial_px_per_second.is_finite() {
        settings.sequence_initial_px_per_second = 80.0;
    }
    if !settings.sequence_initial_lane_height_px.is_finite() {
        settings.sequence_initial_lane_height_px = 42.0;
    }
    if !settings.sequence_spectrogram_time_resolution_ms.is_finite() {
        settings.sequence_spectrogram_time_resolution_ms = 10.0;
    }
    settings.sequence_initial_px_per_second =
        settings.sequence_initial_px_per_second.clamp(20.0, 12000.0);
    settings.sequence_initial_lane_height_px =
        settings.sequence_initial_lane_height_px.clamp(24.0, 120.0);
    settings.sequence_spectrogram_time_resolution_ms = settings
        .sequence_spectrogram_time_resolution_ms
        .clamp(0.5, 100.0);
    settings.sequence_spectrogram_fft_size =
        nearest_power_of_two(settings.sequence_spectrogram_fft_size.clamp(512, 16384));
    settings.effect_raster.max_columns = settings.effect_raster.max_columns.clamp(16, 1024);
    settings.effect_raster.max_rows = settings.effect_raster.max_rows.clamp(1, 200);
    settings.effect_raster.min_frame_stride = settings.effect_raster.min_frame_stride.clamp(1, 16);
    settings
}

fn nearest_power_of_two(value: u32) -> u32 {
    let lower = 1u32 << (31 - value.leading_zeros());
    let upper = lower.saturating_mul(2);
    if value - lower < upper.saturating_sub(value) {
        lower
    } else {
        upper.min(16384)
    }
}
