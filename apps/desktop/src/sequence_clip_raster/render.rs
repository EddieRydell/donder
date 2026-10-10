use donder_runtime::PreparedSequence;
use donder_sequence_api::EffectRasterSettings;
use std::sync::Arc;

pub(super) struct RenderedRaster {
    pub(super) columns: u32,
    pub(super) rows: u32,
    pub(super) pixels_rgba: Arc<[u8]>,
}

pub(super) enum RasterRenderFailure {
    Cancelled,
    Error(String),
}

/// One column per sampled frame, at most `max_columns` and at least
/// `min_frame_stride` frames apart; one row per sampled pixel, at most
/// `max_rows`. The resolution depends only on the clip, never on its size on
/// screen.
pub(super) fn render_effect_raster(
    sequence: &PreparedSequence,
    effect_id: u32,
    settings: &EffectRasterSettings,
    should_continue: &dyn Fn() -> bool,
) -> Result<RenderedRaster, RasterRenderFailure> {
    let clip = sequence
        .clip(effect_id)
        .ok_or_else(|| RasterRenderFailure::Error("the clip is not prepared".into()))?;
    let duration_seconds = donder_runtime_types::sample_duration_seconds_f32(clip.duration());
    let duration_frames = duration_seconds * clip.frame_rate() as f32;
    if !duration_frames.is_finite() || duration_frames <= 0.0 {
        return Err(RasterRenderFailure::Error(
            "effect duration frames must be positive and finite".into(),
        ));
    }
    let target_pixel_count = clip.target_pixel_count();
    if target_pixel_count == 0 {
        return Err(RasterRenderFailure::Error(
            "effect target has no pixels".into(),
        ));
    }
    let min_frame_stride = settings.min_frame_stride.max(1) as f32;
    let columns = ((duration_frames / min_frame_stride).ceil().max(1.0) as u32)
        .clamp(1, settings.max_columns.max(1)) as usize;
    let rows = target_pixel_count.min(settings.max_rows.max(1) as usize);
    let mut sampler = clip.sampler(rows);
    let mut pixels_rgba = vec![0u8; rows * columns * 4];
    for column in 0..columns {
        if !should_continue() {
            return Err(RasterRenderFailure::Cancelled);
        }
        let time = clip
            .raster_column_time(column, columns)
            .map_err(|error| RasterRenderFailure::Error(format!("{error:?}")))?;
        for (row, color) in sampler.evaluate(time).iter().enumerate() {
            let offset = (row * columns + column) * 4;
            pixels_rgba[offset] = color.red;
            pixels_rgba[offset + 1] = color.green;
            pixels_rgba[offset + 2] = color.blue;
            pixels_rgba[offset + 3] = 255;
        }
    }
    Ok(RenderedRaster {
        columns: columns as u32,
        rows: rows as u32,
        pixels_rgba: pixels_rgba.into(),
    })
}
