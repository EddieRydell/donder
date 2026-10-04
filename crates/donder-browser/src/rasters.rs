use super::*;
use donder_sequence_api::BrowserClipRaster;

#[wasm_bindgen]
impl BrowserSession {
    #[wasm_bindgen(js_name = renderClipRaster)]
    pub fn render_clip_raster(
        &self,
        effect_id: u32,
        columns: u32,
        rows: u32,
    ) -> Result<JsValue, JsValue> {
        let clip = self
            .playback
            .sequence()
            .clip(effect_id)
            .ok_or_else(|| JsValue::from_str("The requested effect clip was not found."))?;
        let settings = donder_sequence_api::AppSettings::default().effect_raster;
        let start_seconds = donder_language::values::sample_time_seconds_f32(clip.start_time());
        let duration_seconds =
            donder_language::values::sample_duration_seconds_f32(clip.duration());
        let stride_columns = (duration_seconds * clip.frame_rate() as f32
            / settings.min_frame_stride.max(1) as f32)
            .ceil()
            .max(1.0) as u32;
        let columns = columns
            .min(stride_columns)
            .clamp(1, settings.max_columns.max(1));
        let rows = rows
            .min(settings.max_rows)
            .min(clip.target_pixel_count() as u32)
            .max(1);
        let mut sampler = clip.sampler(rows as usize);
        let mut pixels_rgba = vec![0; columns as usize * rows as usize * 4];
        for column in 0..columns as usize {
            let time = clip
                .raster_column_time(column, columns as usize)
                .map_err(|error| {
                    JsValue::from_str(&format!("Invalid raster sample time: {error:?}"))
                })?;
            for (row, color) in sampler.evaluate(time).iter().enumerate() {
                let offset = (row * columns as usize + column) * 4;
                pixels_rgba[offset..offset + 4].copy_from_slice(&[
                    color.red,
                    color.green,
                    color.blue,
                    255,
                ]);
            }
        }
        js_value(&BrowserClipRaster {
            effect_id,
            columns,
            rows,
            start_seconds,
            duration_seconds,
            pixels_rgba,
        })
    }
}
