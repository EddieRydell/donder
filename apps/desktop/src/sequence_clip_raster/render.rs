use super::*;

pub(super) struct RasterRenderRequest<'a> {
    pub(super) renderer: Arc<PreparedSequence>,
    pub(super) effect_id: u32,
    pub(super) signature_key: &'a str,
    pub(super) cache_key: &'a RasterCacheKey,
    pub(super) display_column_count: u32,
    pub(super) display_row_count: u32,
    pub(super) settings: &'a EffectRasterSettings,
    pub(super) should_continue: &'a dyn Fn() -> bool,
}

pub(super) fn render_effect_raster(
    request: RasterRenderRequest<'_>,
) -> Result<CachedRasterPayload, RasterRenderFailure> {
    let RasterRenderRequest {
        renderer,
        effect_id,
        signature_key,
        cache_key,
        display_column_count,
        display_row_count,
        settings,
        should_continue,
    } = request;
    if display_column_count == 0 {
        return Err(RasterRenderFailure::Error(
            "raster display column count must be greater than zero".to_string(),
        ));
    }
    if display_row_count == 0 {
        return Err(RasterRenderFailure::Error(
            "raster display row count must be greater than zero".to_string(),
        ));
    }
    let renderer = renderer
        .clip(effect_id)
        .ok_or_else(|| RasterRenderFailure::Error("raster clip selection is unavailable".into()))?;
    let start_seconds = donder_language::values::sample_time_seconds_f32(renderer.start_time());
    let duration_seconds =
        donder_language::values::sample_duration_seconds_f32(renderer.duration());
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return Err(RasterRenderFailure::Error(
            "effect duration must be positive and finite".to_string(),
        ));
    }
    let target_pixel_count = renderer.target_pixel_count();
    if target_pixel_count == 0 {
        return Err(RasterRenderFailure::Error(
            "effect target has no pixels".to_string(),
        ));
    }
    let duration_frames = duration_seconds * renderer.frame_rate() as f32;
    if !duration_frames.is_finite() || duration_frames <= 0.0 {
        return Err(RasterRenderFailure::Error(
            "effect duration frames must be positive and finite".to_string(),
        ));
    }
    let min_frame_stride = settings.min_frame_stride.max(1) as f32;
    let stride_limited_columns = (duration_frames / min_frame_stride).ceil().max(1.0) as u32;
    let columns = display_column_count
        .min(stride_limited_columns)
        .clamp(1, settings.max_columns.max(1)) as usize;
    let rows = target_pixel_count
        .min(display_row_count as usize)
        .min(settings.max_rows.max(1) as usize);
    let mut sampler = renderer.sampler(rows);
    let mut pixels_rgba = vec![0u8; rows * columns * 4];
    for column in 0..columns {
        if !should_continue() {
            return Err(RasterRenderFailure::Cancelled);
        }
        let time = raster_column_time(&renderer, column, columns)?;
        let colors = sampler.evaluate(time);
        for (row, color) in colors.iter().enumerate() {
            let offset = (row * columns + column) * 4;
            pixels_rgba[offset] = color.red;
            pixels_rgba[offset + 1] = color.green;
            pixels_rgba[offset + 2] = color.blue;
            pixels_rgba[offset + 3] = 255;
        }
    }
    let token = raster_token(cache_key, signature_key);
    Ok(CachedRasterPayload {
        raster: SequenceClipRaster {
            request_id: 0,
            effect_id,
            signature: String::new(),
            columns: columns as u32,
            rows: rows as u32,
            start_seconds,
            duration_seconds,
            pixels_rgba_token: token.clone(),
        },
        pixels_rgba: Arc::new(pixels_rgba),
        token,
    })
}

pub(super) enum RasterRenderFailure {
    Cancelled,
    Error(String),
}

fn raster_column_time(
    clip: &donder_runtime::SequenceClip<'_>,
    column: usize,
    columns: usize,
) -> Result<donder_language::values::SampleTime, RasterRenderFailure> {
    let rate = u64::from(clip.frame_rate());
    let ticks = u64::from(clip.start_time().as_ticks());
    let end = ticks + u64::from(clip.duration().as_ticks());
    let micros = u64::from(donder_language::values::MICROS_PER_SECOND);
    let start_frame = (ticks * rate).div_ceil(micros);
    let end_frame = (end * rate).div_ceil(micros);
    let active_frames = end_frame.saturating_sub(start_frame).max(1);
    let offset = (((column as f32 + 0.5) * active_frames as f32 / columns as f32).floor() as u64)
        .min(active_frames - 1);
    let frame = (start_frame + offset).min(u64::from(clip.frame_count().saturating_sub(1)));
    donder_language::values::sample_time_from_frame(frame as u32, clip.frame_rate())
        .map_err(|error| RasterRenderFailure::Error(format!("{error:?}")))
}
