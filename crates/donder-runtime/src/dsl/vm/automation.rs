//! Automation: an invocation's parameter copy in the workspace, rewritten
//! from its plan's bindings at each sample time. Words are written directly;
//! curve windows are rewritten in their own buffers. Live controls replace a
//! binding's curve with a constant from their time on.
use crate::resources::{
    LiveView, ParamsView, ResourceSlot, SOURCE_LIVE, SOURCE_WINDOW, WindowState, curve_crossings,
};
use crate::show::Show;
use bytemuck::{Pod, Zeroable};
use donder_runtime_types::image::{self, CrossingSegment, CurvePoint, NONE};
use donder_runtime_types::sampling::{curve_area, curve_prefix_areas, sample_curve_points};
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

/// One automated or live-bound invocation's state: where its buffers start
/// in the shared arrays, and its sample time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct StateRecord {
    pub(crate) plan: u32,
    pub(crate) params: u32,
    pub(crate) words: image::Range,
    pub(crate) resources: image::Range,
    pub(crate) windows: image::Range,
    pub(crate) points: image::Range,
    pub(crate) overrides: image::Range,
    /// Where the program's time starts; a voice moves it to its start.
    pub(crate) origin: u32,
    /// The tick the copy holds, or [`NONE`].
    pub(crate) time: u32,
}

/// Every state's buffers, in state order.
pub(crate) struct States<'w> {
    pub(crate) records: &'w mut [StateRecord],
    pub(crate) words: &'w mut [u32],
    pub(crate) resources: &'w mut [ResourceSlot],
    pub(crate) windows: &'w mut [WindowState],
    pub(crate) points: &'w mut [CurvePoint],
    pub(crate) areas: &'w mut [f32],
    pub(crate) crossings: &'w mut [CrossingSegment],
    pub(crate) overrides: &'w mut [Override],
}

/// What [`States`] holds for one state, borrowed.
pub(crate) struct State<'s> {
    pub(crate) record: &'s mut StateRecord,
    words: &'s mut [u32],
    resources: &'s mut [ResourceSlot],
    windows: &'s mut [WindowState],
    points: &'s mut [CurvePoint],
    areas: &'s mut [f32],
    crossings: &'s mut [CrossingSegment],
    overrides: &'s mut [Override],
}

fn split<T>(items: &mut [T], range: image::Range) -> (&mut [T], &mut [T]) {
    let (before, rest) = items.split_at_mut(range.start as usize);
    let (current, _) = rest.split_at_mut(range.len as usize);
    (before, current)
}

impl<'w> States<'w> {
    /// State `index`, and the states before it.
    // Once per automated operator sample, so it stays in flash.
    #[inline(never)]
    pub(crate) fn split(&mut self, index: usize) -> (States<'_>, State<'_>) {
        let (records, rest) = self.records.split_at_mut(index);
        let record = &mut rest[0];
        let (words, current_words) = split(self.words, record.words);
        let (resources, current_resources) = split(self.resources, record.resources);
        let (windows, current_windows) = split(self.windows, record.windows);
        let (points, current_points) = split(self.points, record.points);
        let (areas, current_areas) = split(self.areas, record.points);
        let (crossings, current_crossings) = split(self.crossings, record.points);
        let (overrides, current_overrides) = split(self.overrides, record.overrides);
        (
            States {
                records,
                words,
                resources,
                windows,
                points,
                areas,
                crossings,
                overrides,
            },
            State {
                record,
                words: current_words,
                resources: current_resources,
                windows: current_windows,
                points: current_points,
                areas: current_areas,
                crossings: current_crossings,
                overrides: current_overrides,
            },
        )
    }

    pub(crate) fn reborrow(&mut self) -> States<'_> {
        States {
            records: self.records,
            words: self.words,
            resources: self.resources,
            windows: self.windows,
            points: self.points,
            areas: self.areas,
            crossings: self.crossings,
            overrides: self.overrides,
        }
    }

    pub(crate) fn get(&mut self, index: usize) -> State<'_> {
        self.split(index).1
    }

    /// Every state samples again: a command changed what they hold.
    pub(crate) fn forget(&mut self) {
        for record in self.records.iter_mut() {
            record.time = NONE;
        }
    }
}

/// The bindings, windows and overrides an invocation's state needs.
pub(crate) fn shape(show: Show<'_>, plan: u32, params: u32) -> (usize, usize, usize, usize, usize) {
    let s = &show.s;
    let set = &s.param_sets[params as usize];
    let (windows, points, bindings) = match show.plan(plan) {
        Some(plan) => (
            plan.windows.len as usize,
            plan.windows
                .get(s.windows)
                .iter()
                .map(|window| window.points.max(1) as usize)
                .sum(),
            plan.bindings.len as usize,
        ),
        None => (0, 0, 0),
    };
    (
        set.words.len as usize,
        set.resources.len as usize,
        windows,
        points,
        bindings,
    )
}

impl<'s> State<'s> {
    /// Start the copy from the image's values; its windows hold nothing yet.
    pub(crate) fn detach(&mut self, show: Show<'_>) {
        let s = &show.s;
        let set = &s.param_sets[self.record.params as usize];
        self.words.copy_from_slice(set.words.get(s.words));
        for (slot, resource) in self
            .resources
            .iter_mut()
            .zip(set.resources.get(s.resources))
        {
            *slot = ResourceSlot::image(*resource);
        }
        let mut start = 0;
        if let Some(plan) = show.plan(self.record.plan) {
            for (index, (window, state)) in plan
                .windows
                .get(s.windows)
                .iter()
                .zip(self.windows.iter_mut())
                .enumerate()
            {
                let capacity = window.points.max(1);
                *state = WindowState {
                    start,
                    capacity,
                    len: 0,
                    kind: image::CROSSINGS_INCREASING,
                };
                start += capacity;
                self.resources[window.resource as usize] = ResourceSlot {
                    kind: image::RESOURCE_CURVE,
                    source: SOURCE_WINDOW,
                    index: index as u32,
                    origin: 0,
                };
            }
        }
        self.overrides.fill(Override::default());
        self.record.time = NONE;
    }

    /// Bind resource `bank` to live collection `collection`, its marks
    /// measured from `origin`.
    pub(crate) fn bind_live(&mut self, bank: usize, collection: u32, origin: u32) {
        self.resources[bank] = ResourceSlot {
            kind: image::RESOURCE_MARKS,
            source: SOURCE_LIVE,
            index: collection,
            origin,
        };
        self.record.time = NONE;
    }

    /// Measure the program's time from `origin`, as when its clip starts there.
    pub(crate) fn restart_at(&mut self, origin: u32) {
        self.record.origin = origin;
        for item in self.overrides.iter_mut() {
            item.area_offset = 0.0;
        }
        self.record.time = NONE;
    }

    /// The copy at `time`, rewritten from the plan when it holds another.
    // Once per invocation and sample time, so it stays in flash.
    #[inline(never)]
    pub(crate) fn params_at(
        mut self,
        show: Show<'_>,
        time: SampleTime,
        live: LiveView<'s>,
    ) -> ParamsView<'s> {
        if self.record.time != time.as_ticks() {
            self.apply(show, time);
            self.record.time = time.as_ticks();
        }
        ParamsView::copy(
            self.words,
            self.resources,
            self.windows,
            self.points,
            self.areas,
            self.crossings,
            live,
        )
    }

    /// Make `binding` hold `value` (in `0..=1`) from `time` on. An integral
    /// keeps the area it had reached.
    pub(crate) fn set_value(
        &mut self,
        show: Show<'_>,
        binding: usize,
        value: f32,
        time: SampleTime,
    ) {
        let Some(plan) = show.plan(self.record.plan) else {
            return;
        };
        let Some(record) = plan.bindings.get(show.s.bindings).get(binding) else {
            return;
        };
        let origin = self.record.origin;
        let position =
            |at: u32| (i64::from(at) - i64::from(record.start)) as f32 / record.duration as f32;
        let integral = record.kind == image::BINDING_FLOAT_INTEGRAL;
        let before = integral.then(|| {
            area(
                show,
                record,
                &self.overrides[binding],
                position(origin),
                position(time.as_ticks()),
            )
        });
        let item = &mut self.overrides[binding];
        item.active = 1;
        item.value = value;
        if let Some(before) = before {
            item.area_offset += before
                - area(
                    show,
                    record,
                    item,
                    position(origin),
                    position(time.as_ticks()),
                );
        }
        self.record.time = NONE;
    }

    /// Apply in authored order, preserving last-binding-wins behavior.
    #[inline(never)]
    fn apply(&mut self, show: Show<'_>, time: SampleTime) {
        let Some(plan) = show.plan(self.record.plan) else {
            return;
        };
        let s = &show.s;
        let ticks = time.as_ticks();
        let origin = self.record.origin;
        for (binding, record) in plan.bindings.get(s.bindings).iter().enumerate() {
            let item = self.overrides[binding];
            let elapsed = ticks.saturating_sub(record.start);
            let position = (elapsed as f32 / record.duration as f32).clamp(0.0, 1.0);
            let amount = || {
                if item.active != 0 {
                    item.value
                } else {
                    sample_curve_points(show.curve(record.curve).points, position)
                }
                .clamp(0.0, 1.0)
            };
            let destination = record.destination as usize;
            let (fmin, fmax) = (f32::from_bits(record.min), f32::from_bits(record.max));
            match record.kind {
                image::BINDING_FLOAT => {
                    self.words[destination] = (fmin + (fmax - fmin) * amount()).to_bits();
                }
                image::BINDING_FLOAT_INTEGRAL => {
                    // Admission keeps automation values in 0..1, so the
                    // value's clamp never applies and the area is exact.
                    let span = record.duration as f32;
                    let at = |tick: u32| (i64::from(tick) - i64::from(record.start)) as f32 / span;
                    let area = area(show, record, &item, at(origin), at(ticks)) + item.area_offset;
                    let elapsed = (i64::from(ticks) - i64::from(origin)) as f32 * SECONDS_PER_TICK;
                    let seconds = span * SECONDS_PER_TICK;
                    self.words[destination] =
                        (fmin * elapsed + (fmax - fmin) * seconds * area).to_bits();
                }
                image::BINDING_INT => {
                    let (min, max) = (record.min as i32 as f32, record.max as i32 as f32);
                    self.words[destination] =
                        libm::roundf(min + (max - min) * amount()) as i32 as u32;
                }
                image::BINDING_BOOL => {
                    self.words[destination] = u32::from(amount() >= 0.5);
                }
                image::BINDING_ENUM => {
                    let options = record.options.get(s.enum_options);
                    let index = (libm::floorf(amount() * options.len() as f32) as usize)
                        .min(options.len() - 1);
                    self.words[destination] = options[index];
                }
                _ => {
                    let window = &mut self.windows[destination];
                    let range = window.start as usize..(window.start + window.capacity) as usize;
                    let constant = [CurvePoint {
                        position: 0.0,
                        value: item.value,
                    }];
                    let source = if item.active != 0 {
                        &constant[..]
                    } else {
                        show.curve(record.curve).points
                    };
                    let len = curve_window_points(
                        &mut self.points[range.clone()],
                        source,
                        fmin,
                        fmax,
                        position,
                    );
                    let points = &self.points[window.start as usize..window.start as usize + len];
                    curve_prefix_areas(points, &mut self.areas[window.start as usize..]);
                    let (kind, _) = curve_crossings(points, &mut self.crossings[range]);
                    window.len = len as u32;
                    window.kind = kind;
                }
            }
        }
    }
}

/// The area under a binding's curve, or its live constant, between two
/// positions of the binding.
fn area(show: Show<'_>, record: &image::Binding, item: &Override, from: f32, to: f32) -> f32 {
    if item.active != 0 {
        let constant = [CurvePoint {
            position: 0.0,
            value: item.value,
        }];
        curve_area(&constant, from, to)
    } else {
        curve_area(show.curve(record.curve).points, from, to)
    }
}
