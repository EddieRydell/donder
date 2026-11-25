//! Bound parameters and the resources they hold, resolved against a show,
//! and a console's live store and live mark collections.
use crate::format::{GradientStop, NONE};
use bytemuck::{Pod, Zeroable};
use donder_runtime_types::sampling::CurveView;
use donder_runtime_types::{
    FontView, GradientSpace, ImageView, MarkDurations, MarkLabels, MarkWindow, SampleDuration,
    SampleTime, TrackView,
};

use crate::view::Show;

/// A live collection's marks, in ticks ascending, in the shared buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct LiveTrack {
    pub(crate) start: u32,
    pub(crate) len: u32,
}

/// A console's live state as programs read it: every live collection's
/// marks, and the live store's words.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LiveView<'a> {
    pub(crate) tracks: &'a [LiveTrack],
    pub(crate) ticks: &'a [u32],
    pub(crate) values: &'a [f32],
}

/// A program's bound parameters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ParamsView<'a> {
    pub(crate) words: &'a [u32],
    pub(crate) resources: &'a [u32],
    pub(crate) live: LiveView<'a>,
}

impl<'a> ParamsView<'a> {
    /// Parameter set `set` of the show, reading `live`.
    pub(crate) fn show(show: Show<'a>, set: u32, live: LiveView<'a>) -> Self {
        let s = &show.s;
        let set = &s.param_sets[set as usize];
        Self {
            words: set.words.get(s.words),
            resources: set.resources.get(s.resources),
            live,
        }
    }

    /// Show marks `index`, or the live collection it reads, measured from
    /// `origin`.
    #[inline(never)]
    pub(crate) fn marks(&self, show: Show<'a>, index: u32, origin: SampleTime) -> MarkWindow<'a> {
        let collection = show.s.marks[index as usize].live;
        if collection == NONE {
            return show.marks(index);
        }
        let origin = origin.as_ticks();
        let track = self.live.tracks[collection as usize];
        let ticks = &self.live.ticks[track.start as usize..(track.start + track.len) as usize];
        let first = ticks.partition_point(|&mark| mark < origin);
        MarkWindow {
            ticks: &ticks[first..],
            durations: MarkDurations::Points,
            labels: MarkLabels {
                codes: &[],
                spans: &[],
            },
            origin,
        }
    }
}

impl<'a> Show<'a> {
    #[inline(never)]
    pub(crate) fn curve(&self, index: u32) -> CurveView<'a> {
        let s = &self.s;
        let curve = &s.curves[index as usize];
        CurveView {
            points: curve.points.get(s.curve_points),
            areas: curve.areas.get(s.curve_areas),
            slopes: curve.slopes.get(s.curve_slopes),
            kind: curve.crossings_kind,
            crossings: curve.crossings.get(s.crossings),
        }
    }

    #[inline(never)]
    pub(crate) fn gradient(&self, index: u32) -> (GradientSpace, &'a [GradientStop]) {
        let s = &self.s;
        let gradient = &s.gradients[index as usize];
        (gradient.space(), gradient.stops.get(s.gradient_stops))
    }

    #[inline(never)]
    pub(crate) fn marks(&self, index: u32) -> MarkWindow<'a> {
        let s = &self.s;
        let marks = &s.marks[index as usize];
        let window = marks.first as usize..marks.end as usize;
        MarkWindow {
            ticks: &marks.track.get(s.mark_ticks)[window.clone()],
            durations: match marks.durations.len {
                0 => MarkDurations::Points,
                _ => MarkDurations::Spans(&marks.durations.get(s.mark_ticks)[window.clone()]),
            },
            labels: MarkLabels {
                codes: marks.label_codes.get(s.mark_ticks),
                spans: match marks.label_spans.len {
                    0 => &[],
                    _ => &marks.label_spans.get(s.mark_ticks)[2 * window.start..2 * window.end],
                },
            },
            origin: marks.origin,
        }
    }

    #[inline(never)]
    pub(crate) fn font(&self, index: u32) -> FontView<'a> {
        let s = &self.s;
        let font = &s.fonts[index as usize];
        FontView {
            height: font.height,
            glyphs: font.glyphs.get(s.glyphs),
            coverage: font.coverage.get(s.glyph_coverage),
        }
    }

    #[inline(never)]
    pub(crate) fn image(&self, index: u32) -> ImageView<'a> {
        let s = &self.s;
        let image = &s.images[index as usize];
        ImageView {
            width: image.width,
            height: image.height,
            frames: image.frames,
            rate: image.rate,
            pixels: image.pixels.get(s.image_pixels),
        }
    }

    #[inline(never)]
    pub(crate) fn audio(&self, index: u32) -> TrackView<'a> {
        let s = &self.s;
        let track = &s.audio_tracks[index as usize];
        TrackView {
            samples: track.samples.get(s.audio_samples),
            channels: track.channels as u16,
            signed: track.signed != 0,
            rate: track.rate as f32,
            origin: donder_runtime_types::sample_duration_seconds_f32(SampleDuration::from_ticks(
                track.origin,
            )),
        }
    }

    #[inline(never)]
    pub(crate) fn array(&self, index: u32) -> &'a [u32] {
        let s = &self.s;
        s.arrays[index as usize].items.get(s.values)
    }
}
