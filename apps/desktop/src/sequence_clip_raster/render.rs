use donder_editor::dto::ClipRasterSettings;
use donder_runtime::PreparedShow;
use std::sync::Arc;

pub(super) struct RenderedRaster {
    pub(super) columns: u32,
    pub(super) rows: u32,
    pub(super) pixels_rgba: Arc<[u8]>,
}

/// The job was superseded before the clip finished.
pub(super) struct Cancelled;

/// One column per frame of the clip; one row per sampled pixel, at most
/// `max_rows`. The resolution depends only on the clip, never on its size on
/// screen.
pub(super) fn render_clip_raster(
    sequence: &PreparedShow,
    clip_id: u32,
    settings: &ClipRasterSettings,
    should_continue: &dyn Fn() -> bool,
) -> Result<RenderedRaster, Cancelled> {
    let Some(clip) = sequence.clip(clip_id) else {
        unreachable!("the worker renders clips of the show it prepared from their session")
    };
    let frames = clip.raster_frames();
    let columns = frames.len();
    let rows = clip
        .target_pixel_count()
        .min(settings.max_rows.max(1) as usize);
    let mut raster = clip.raster(rows);
    let mut pixels_rgba = vec![0u8; rows * columns * 4];
    for (column, frame) in frames.enumerate() {
        if !should_continue() {
            return Err(Cancelled);
        }
        for (row, color) in raster.evaluate(clip.frame_time(frame)).iter().enumerate() {
            let offset = (row * columns + column) * 4;
            pixels_rgba[offset..offset + 3].copy_from_slice(&color.to_bytes());
            pixels_rgba[offset + 3] = 255;
        }
    }
    Ok(RenderedRaster {
        columns: columns as u32,
        rows: rows as u32,
        pixels_rgba: pixels_rgba.into(),
    })
}
