//! Bound parameters and the resources they hold, resolved against a show.
//! A parameter set is the image's, or an automated or live-bound copy in the
//! workspace whose curve windows and live marks are workspace buffers.
use crate::image::{
    self, CROSSINGS_DECREASING, CROSSINGS_INCREASING, CrossingSegment, CurvePoint, GradientStop,
    NONE, ResourceRef,
};
use bytemuck::{Pod, Zeroable};
use donder_runtime_types::{MarkWindow, sampling};

use crate::show::Show;

/// A curve: its points and the segments that invert it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CurveView<'a> {
    pub(crate) points: &'a [CurvePoint],
    pub(crate) areas: &'a [f32],
    pub(crate) slopes: &'a [f32],
    pub(crate) kind: u32,
    pub(crate) crossings: &'a [CrossingSegment],
}

impl<'a> CurveView<'a> {
    pub(crate) fn sample(self, position: f32) -> f32 {
        sampling::sample_curve_sloped(self.points, |index| self.slopes[index], position)
    }

    pub(crate) fn integral(self, position: f32) -> f32 {
        sampling::curve_integral_table(self.points, self.areas, position)
    }

    pub(crate) fn last_crossing(self, value: f32, before: f32) -> f32 {
        sampling::curve_last_crossing(self.points, value, before)
    }

    /// The first position where the curve reaches `value`, or NaN. Rising or
    /// falling curves search their segments; others scan them.
    pub(crate) fn crossing(self, value: f32) -> f32 {
        let fallback = f32::NAN;
        let crossing = |segment: &CrossingSegment| {
            if value >= segment.min_value && value <= segment.max_value {
                Some(segment.position_bias + value * segment.position_scale)
            } else {
                None
            }
        };
        let segments = self.crossings;
        match segments {
            [] => return fallback,
            [segment] => return crossing(segment).unwrap_or(fallback),
            _ => {}
        }
        match self.kind {
            CROSSINGS_INCREASING => {
                let index = segments.partition_point(|segment| segment.max_value < value);
                segments.get(index).and_then(crossing).unwrap_or(fallback)
            }
            CROSSINGS_DECREASING => {
                let index = segments.partition_point(|segment| segment.min_value > value);
                segments.get(index).and_then(crossing).unwrap_or(fallback)
            }
            _ => segments.iter().find_map(crossing).unwrap_or(fallback),
        }
    }
}

/// The crossing segments of `points` into `output`, at least
/// `points.len().max(1)` long; returns their kind and count.
pub(crate) fn curve_crossings(
    points: &[CurvePoint],
    output: &mut [CrossingSegment],
) -> (u32, usize) {
    let mut increasing = true;
    let mut decreasing = true;
    let mut len = 0;
    for pair in points.windows(2) {
        let (start, end) = (&pair[0], &pair[1]);
        increasing &= start.value <= end.value;
        decreasing &= start.value >= end.value;
        let span = end.value - start.value;
        let position_scale = if span == 0.0 {
            0.0
        } else {
            (end.position - start.position) / span
        };
        output[len] = CrossingSegment {
            position_bias: start.position - start.value * position_scale,
            position_scale,
            min_value: start.value.min(end.value),
            max_value: start.value.max(end.value),
        };
        len += 1;
    }
    if let [point] = points {
        output[0] = CrossingSegment {
            position_bias: point.position,
            position_scale: 0.0,
            min_value: point.value,
            max_value: point.value,
        };
        len = 1;
    }
    let kind = if increasing {
        CROSSINGS_INCREASING
    } else if decreasing {
        CROSSINGS_DECREASING
    } else {
        image::CROSSINGS_MIXED
    };
    (kind, len)
}

/// Where a bound resource lives.
pub(crate) const SOURCE_IMAGE: u32 = 0;
/// A curve window its automation rewrites.
pub(crate) const SOURCE_WINDOW: u32 = 1;
/// A live mark collection, measured from `origin`.
pub(crate) const SOURCE_LIVE: u32 = 2;

/// A resource of a workspace parameter copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct ResourceSlot {
    pub(crate) kind: u32,
    pub(crate) source: u32,
    /// The image resource, the window, or the live collection.
    pub(crate) index: u32,
    pub(crate) origin: u32,
}

impl ResourceSlot {
    pub(crate) fn image(resource: ResourceRef) -> Self {
        Self {
            kind: resource.kind,
            source: SOURCE_IMAGE,
            index: resource.index,
            origin: 0,
        }
    }
}

/// A curve window's place in its parameter copy's buffers and what it holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct WindowState {
    pub(crate) start: u32,
    pub(crate) capacity: u32,
    pub(crate) len: u32,
    pub(crate) kind: u32,
}

/// A live collection's marks, in ticks ascending, in the shared buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct LiveTrack {
    pub(crate) start: u32,
    pub(crate) len: u32,
}

/// Every live collection's marks.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LiveView<'a> {
    pub(crate) tracks: &'a [LiveTrack],
    pub(crate) ticks: &'a [u32],
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ResourceList<'a> {
    Image(&'a [ResourceRef]),
    Slots(&'a [ResourceSlot]),
}

/// A program's bound parameters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ParamsView<'a> {
    pub(crate) words: &'a [u32],
    pub(crate) resources: ResourceList<'a>,
    pub(crate) windows: &'a [WindowState],
    pub(crate) points: &'a [CurvePoint],
    pub(crate) areas: &'a [f32],
    pub(crate) slopes: &'a [f32],
    pub(crate) crossings: &'a [CrossingSegment],
    pub(crate) live: LiveView<'a>,
}

impl<'a> ParamsView<'a> {
    /// Parameter set `set` as the image binds it.
    pub(crate) fn image(show: Show<'a>, set: u32) -> Self {
        let s = &show.s;
        let set = &s.param_sets[set as usize];
        Self {
            words: set.words.get(s.words),
            resources: ResourceList::Image(set.resources.get(s.resources)),
            windows: &[],
            points: &[],
            areas: &[],
            slopes: &[],
            crossings: &[],
            live: LiveView::default(),
        }
    }

    /// Resource parameter `bank`.
    pub(crate) fn resource(&self, bank: u16) -> ResourceSlot {
        match self.resources {
            ResourceList::Image(resources) => ResourceSlot::image(resources[usize::from(bank)]),
            ResourceList::Slots(slots) => slots[usize::from(bank)],
        }
    }

    // Once per resource instruction, so it stays in flash.
    #[inline(never)]
    pub(crate) fn curve(&self, show: Show<'a>, slot: ResourceSlot) -> CurveView<'a> {
        if slot.source == SOURCE_WINDOW {
            let window = &self.windows[slot.index as usize];
            let range = window.start as usize..(window.start + window.len) as usize;
            return CurveView {
                points: &self.points[range.clone()],
                areas: &self.areas[range.clone()],
                slopes: &self.slopes[range.clone()],
                kind: window.kind,
                crossings: &self.crossings[range],
            };
        }
        show.curve(slot.index)
    }

    #[inline(never)]
    pub(crate) fn marks(&self, show: Show<'a>, slot: ResourceSlot) -> MarkWindow<'a> {
        if slot.source == SOURCE_LIVE {
            let track = self.live.tracks[slot.index as usize];
            let ticks = &self.live.ticks[track.start as usize..(track.start + track.len) as usize];
            let first = ticks.partition_point(|&mark| mark < slot.origin);
            return MarkWindow {
                ticks: &ticks[first..],
                origin: slot.origin,
            };
        }
        show.marks(slot.index)
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
    pub(crate) fn gradient(&self, index: u32) -> &'a [GradientStop] {
        let s = &self.s;
        s.gradients[index as usize].stops.get(s.gradient_stops)
    }

    #[inline(never)]
    pub(crate) fn marks(&self, index: u32) -> MarkWindow<'a> {
        let s = &self.s;
        let marks = &s.marks[index as usize];
        let track = marks.track.get(s.tracks);
        MarkWindow {
            ticks: &track[marks.first as usize..marks.end as usize],
            origin: marks.origin,
        }
    }

    #[inline(never)]
    pub(crate) fn array(&self, index: u32) -> &'a [image::Value] {
        let s = &self.s;
        s.arrays[index as usize].items.get(s.values)
    }

    /// The image resource of an automation plan's window, or none.
    pub(crate) fn plan(&self, index: u32) -> Option<&'a image::Automation> {
        (index != NONE).then(|| &self.s.automation[index as usize])
    }
}
