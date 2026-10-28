//! Automation: an invocation's parameters at a time, written into the one
//! scratch copy the running program reads: words from its plan's bindings,
//! curve windows into their own buffers. A rig's controls replace a binding's
//! curve with a constant from when they are set.
use crate::format::{self, CrossingSegment, CurvePoint};
use crate::image::Image;
use crate::resources::{
    LiveView, ParamsView, ResourceList, ResourceSlot, STORE_LIVE, STORE_WINDOW, WindowState,
    curve_crossings,
};
use bytemuck::{Pod, Zeroable};
use donder_runtime_types::sampling::{
    curve_area, curve_prefix_areas, curve_slopes, sample_curve_points,
};
use donder_runtime_types::{SECONDS_PER_TICK, SampleTime, curve_window_points};

/// A live control's value for one binding, from when it was set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Override {
    pub(crate) active: u32,
    pub(crate) value: f32,
    /// Normalized area an integral adds to its curve's, so a value set
    /// mid-session keeps the integral continuous.
    pub(crate) area_offset: f32,
}

/// The copy of the running invocation's parameters.
pub(crate) struct Scratch<'w> {
    pub(crate) words: &'w mut [u32],
    pub(crate) resources: &'w mut [ResourceSlot],
    pub(crate) windows: &'w mut [WindowState],
    pub(crate) points: &'w mut [CurvePoint],
    pub(crate) crossings: &'w mut [CrossingSegment],
    pub(crate) areas: &'w mut [f32],
    pub(crate) slopes: &'w mut [f32],
}

/// What an invocation needs of the scratch: words, resources, curve windows
/// and window points.
pub(crate) fn shape(image: Image<'_>, plan: u32, params: u32) -> [usize; 4] {
    let s = &image.s;
    let set = &s.param_sets[params as usize];
    let (windows, points) = match image.plan(plan) {
        Some(plan) => (
            plan.windows.len as usize,
            plan.windows
                .get(s.windows)
                .iter()
                .map(|window| window.points.max(1) as usize)
                .sum(),
        ),
        None => (0, 0),
    };
    [
        set.words.len as usize,
        set.resources.len as usize,
        windows,
        points,
    ]
}

/// A program's parameters: its bound parameter set, its automation plan
/// or [`NONE`], and the tick its time starts at.
#[derive(Clone, Copy)]
pub(crate) struct ParamsRef {
    pub(crate) params: u32,
    pub(crate) plan: u32,
    pub(crate) origin: u32,
}

/// The invocation's parameters at `time`: the image's when nothing rewrites
/// them, or the scratch copy with automation and live marks applied.
/// `overrides` are the controls of the plan's bindings, or empty.
pub(crate) fn params<'s>(
    image: Image<'s>,
    scratch: &'s mut Scratch<'_>,
    params_ref: ParamsRef,
    time: SampleTime,
    overrides: &[Override],
    live: LiveView<'s>,
) -> ParamsView<'s> {
    let s = &image.s;
    let Some(plan) = image.plan(params_ref.plan) else {
        return ParamsView::image(image, params_ref.params);
    };
    let set = &s.param_sets[params_ref.params as usize];
    let words = set.words.len as usize;
    let resources = set.resources.len as usize;
    scratch.words[..words].copy_from_slice(set.words.get(s.words));
    for (slot, resource) in scratch
        .resources
        .iter_mut()
        .zip(set.resources.get(s.resources))
    {
        *slot = ResourceSlot::image(*resource);
    }
    let mut start = 0;
    for (index, window) in plan.windows.get(s.windows).iter().enumerate() {
        let capacity = window.points.max(1);
        scratch.windows[index] = WindowState {
            start,
            capacity,
            len: 0,
            kind: format::CROSSINGS_INCREASING,
        };
        start += capacity;
        scratch.resources[window.resource as usize] = ResourceSlot {
            kind: format::RESOURCE_CURVE,
            store: STORE_WINDOW,
            index: index as u32,
            origin: 0,
        };
    }
    let window_count = plan.windows.len as usize;
    let point_count = start as usize;
    apply(image, scratch, plan, params_ref.origin, time, overrides);
    ParamsView {
        words: &scratch.words[..words],
        resources: ResourceList::Slots(&scratch.resources[..resources]),
        windows: &scratch.windows[..window_count],
        points: &scratch.points[..point_count],
        areas: &scratch.areas[..point_count],
        slopes: &scratch.slopes[..point_count],
        crossings: &scratch.crossings[..point_count],
        live,
    }
}

/// Apply the plan's bindings in authored order, so the last binding of a
/// parameter wins.
fn apply(
    image: Image<'_>,
    scratch: &mut Scratch<'_>,
    plan: &format::Automation,
    origin: u32,
    time: SampleTime,
    overrides: &[Override],
) {
    let s = &image.s;
    let ticks = time.as_ticks();
    for (binding, record) in plan.bindings.get(s.bindings).iter().enumerate() {
        let item = overrides.get(binding).copied().unwrap_or_default();
        let elapsed = ticks.saturating_sub(record.start);
        let position = (elapsed as f32 / record.duration as f32).clamp(0.0, 1.0);
        let amount = || {
            if item.active != 0 {
                item.value
            } else {
                sample_curve_points(image.curve(record.curve).points, position)
            }
        };
        let destination = record.destination as usize;
        let (fmin, fmax) = (f32::from_bits(record.min), f32::from_bits(record.max));
        match record.kind {
            format::BINDING_FLOAT => {
                scratch.words[destination] = (fmin + (fmax - fmin) * amount()).to_bits();
            }
            format::BINDING_FLOAT_INTEGRAL => {
                // Automation values stay in 0..1, so the value's clamp never
                // applies and the area is exact.
                let span = record.duration as f32;
                let at = |tick: u32| ticks_between(record.start, tick) / span;
                let area = area(image, record, &item, at(origin), at(ticks)) + item.area_offset;
                let elapsed = ticks_between(origin, ticks) * SECONDS_PER_TICK;
                let seconds = span * SECONDS_PER_TICK;
                scratch.words[destination] =
                    (fmin * elapsed + (fmax - fmin) * seconds * area).to_bits();
            }
            format::BINDING_INT => {
                let (min, max) = (record.min as i32 as f32, record.max as i32 as f32);
                scratch.words[destination] =
                    libm::roundf(min + (max - min) * amount()) as i32 as u32;
            }
            format::BINDING_BOOL => {
                scratch.words[destination] = u32::from(amount() >= 0.5);
            }
            format::BINDING_ENUM => {
                let options = record.options.get(s.enum_options);
                let index =
                    (libm::floorf(amount() * options.len() as f32) as usize).min(options.len() - 1);
                scratch.words[destination] = options[index];
            }
            format::BINDING_LIVE_MARKS => {
                scratch.resources[destination] = ResourceSlot {
                    kind: format::RESOURCE_MARKS,
                    store: STORE_LIVE,
                    index: record.curve,
                    origin,
                };
            }
            _ => {
                let window = &mut scratch.windows[destination];
                let range = window.start as usize..(window.start + window.capacity) as usize;
                let constant = [CurvePoint {
                    position: 0.0,
                    value: item.value,
                }];
                let source = if item.active != 0 {
                    &constant[..]
                } else {
                    image.curve(record.curve).points
                };
                let len = curve_window_points(
                    &mut scratch.points[range.clone()],
                    source,
                    fmin,
                    fmax,
                    position,
                );
                let points = &scratch.points[range.start..range.start + len];
                let (kind, _) = curve_crossings(points, &mut scratch.crossings[range.clone()]);
                curve_prefix_areas(points, &mut scratch.areas[range.clone()]);
                curve_slopes(points, &mut scratch.slopes[range]);
                window.len = len as u32;
                window.kind = kind;
            }
        }
    }
}

/// Make `binding` of `plan` hold `value` (in `0..=1`) from `time` on. An
/// integral keeps the area it had reached.
pub(crate) fn set_value(
    image: Image<'_>,
    plan: &format::Automation,
    overrides: &mut [Override],
    binding: usize,
    value: f32,
    time: SampleTime,
    origin: u32,
) {
    let Some(record) = plan.bindings.get(image.s.bindings).get(binding) else {
        return;
    };
    let position = |at: u32| ticks_between(record.start, at) / record.duration as f32;
    let integral = record.kind == format::BINDING_FLOAT_INTEGRAL;
    let before = integral.then(|| {
        area(
            image,
            record,
            &overrides[binding],
            position(origin),
            position(time.as_ticks()),
        )
    });
    let item = &mut overrides[binding];
    item.active = 1;
    item.value = value;
    if let Some(before) = before {
        item.area_offset += before
            - area(
                image,
                record,
                item,
                position(origin),
                position(time.as_ticks()),
            );
    }
}

/// The area under a binding's curve, or its live constant, between two
/// positions of the binding.
fn area(image: Image<'_>, record: &format::Binding, item: &Override, from: f32, to: f32) -> f32 {
    if item.active != 0 {
        let constant = [CurvePoint {
            position: 0.0,
            value: item.value,
        }];
        curve_area(&constant, from, to)
    } else {
        curve_area(image.curve(record.curve).points, from, to)
    }
}

/// Ticks from `from` to `to`, negative when `to` is earlier.
fn ticks_between(from: u32, to: u32) -> f32 {
    if to >= from {
        (to - from) as f32
    } else {
        -((from - to) as f32)
    }
}
