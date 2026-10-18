//! A show's playback workspace, carved from one buffer: the nodes' frame
//! buffers, their state buffers, the one strip workspace programs run in, scan
//! scratch frames, the automation scratch and a rig's state. [`Layout`]
//! counts every region once; [`Layout::size`] and [`carve`] walk them in the
//! same order.
use bytemuck::{Pod, Zeroable};
use donder_runtime_types::image::{self, CrossingSegment, CurvePoint, NONE};
use donder_runtime_types::{Color, SampleTime};

use crate::arena::{Carve, Size};
use crate::dsl::{Override, STRIP, Scratch, StripSlots, StripWorkspace, scratch_shape};
use crate::resources::{LiveTrack, ResourceSlot, WindowState};
use crate::show::Show;

/// The most frame buffers a show's nodes use at once.
pub(crate) const MAX_BUFFERS: usize = 16;
/// The most nodes of a show that keep their previous frame.
pub(crate) const MAX_STATES: usize = 16;
/// The most whole-frame scans one program runs.
pub(crate) const MAX_SCANS: usize = 8;
/// The most marks a live collection keeps; older marks are dropped.
pub const LIVE_MARK_CAPACITY: usize = 1024;
/// A dormant voice starts at the end of time, so it is never active and
/// sorts after every started effect of its layer.
pub(crate) const DORMANT: u32 = u32::MAX;

/// An effect sample kept for duplicate fixture layouts.
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct CachedEffectSample {
    pub(crate) pixel_count: u32,
    pub(crate) color: Color,
}

/// An effect's start and duration in ticks; a rig's commands move them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Timing {
    pub(crate) start: u32,
    pub(crate) duration: u32,
}

impl Timing {
    #[inline(always)]
    pub(crate) fn is_active(self, time: SampleTime) -> bool {
        let ticks = time.as_ticks();
        ticks >= self.start && ticks - self.start < self.duration
    }
}

/// What a rig's commands change: effect timing, each layer's start order,
/// control values and live marks. Empty for a sequence, but for live marks.
pub(crate) struct RigState<'w> {
    pub(crate) timing: &'w mut [Timing],
    pub(crate) layer_order: &'w mut [u32],
    pub(crate) layer_ends: &'w mut [u32],
    /// One per automation binding of the image.
    pub(crate) overrides: &'w mut [Override],
    pub(crate) live_tracks: &'w mut [LiveTrack],
    pub(crate) live_ticks: &'w mut [u32],
}

pub(crate) struct Workspace<'w> {
    pub(crate) buffers: [&'w mut [Color]; MAX_BUFFERS],
    pub(crate) states: [&'w mut [Color]; MAX_STATES],
    pub(crate) scans: [&'w mut [Color]; MAX_SCANS],
    pub(crate) outputs: &'w mut [u8],
    pub(crate) strip: StripWorkspace<'w>,
    pub(crate) scratch: Scratch<'w>,
    pub(crate) gather: &'w mut [[u32; 2]],
    pub(crate) effect_samples: &'w mut [CachedEffectSample],
    pub(crate) rig: RigState<'w>,
    /// The frame the buffers last held, or [`NONE`], and whether they still
    /// hold it: a command since makes them stale.
    pub(crate) frame: &'w mut [u32; 2],
}

/// A show's playback buffer: every region's count, derived from the show
/// once.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pixels: usize,
    output_bytes: usize,
    buffers: usize,
    states: usize,
    scans: usize,
    slots: StripSlots,
    scratch: [usize; 4],
    sample_count: usize,
    rig: bool,
    effects: usize,
    layer_order: usize,
    bindings: usize,
    collections: usize,
}

impl Show<'_> {
    /// The playback buffer this show needs.
    pub fn layout(&self) -> Layout {
        Layout::of(*self)
    }
}

impl Layout {
    pub(crate) fn of(show: Show<'_>) -> Self {
        let s = &show.s;
        let meta = show.meta();
        let mut slots = StripSlots::default();
        let mut scans = 0;
        for program in 0..s.programs.len() {
            let program = show.program(program as u32);
            slots.include(program);
            scans = scans.max(program.scans());
        }
        let mut scratch = [0; 4];
        let invocations = s
            .effects
            .iter()
            .map(|effect| (effect.automation, effect.params))
            .chain(
                s.nodes
                    .iter()
                    .filter(|node| node.kind == image::NODE_OPERATOR)
                    .map(|node| (node.automation, node.params)),
            );
        for (plan, params) in invocations {
            for (total, need) in scratch.iter_mut().zip(scratch_shape(show, plan, params)) {
                *total = (*total).max(need);
            }
        }
        let rig = show.is_rig();
        Self {
            pixels: show.pixel_count(),
            output_bytes: s.outputs.iter().map(|output| output.width as usize).sum(),
            buffers: meta.buffers as usize,
            states: meta.states as usize,
            scans,
            slots,
            scratch,
            sample_count: s
                .effects
                .iter()
                .map(|effect| s.targets[effect.target as usize].sample_count as usize)
                .max()
                .unwrap_or(0),
            rig,
            effects: if rig { s.effects.len() } else { 0 },
            layer_order: if rig { s.layer_order.len() } else { 0 },
            bindings: if rig { s.bindings.len() } else { 0 },
            collections: s.collections.len(),
        }
    }

    /// 32-bit words of the buffer a playback of the show needs.
    pub fn words(&self) -> usize {
        self.size().div_ceil(4)
    }

    /// Bytes of the buffer [`carve`] needs.
    pub(crate) fn size(&self) -> usize {
        let mut size = Size::default();
        for _ in 0..self.buffers + self.states + self.scans {
            size.values::<Color>(self.pixels);
        }
        size.values::<u8>(self.output_bytes);
        self.slots.size(&mut size);
        scratch_size(&mut size, self.scratch);
        size.values::<[u32; 2]>(STRIP);
        size.values::<CachedEffectSample>(self.sample_count);
        size.values::<Timing>(self.effects);
        size.values::<u32>(self.layer_order);
        size.values::<u32>(self.layer_order);
        size.values::<Override>(self.bindings);
        size.values::<LiveTrack>(self.collections);
        size.values::<u32>(self.collections * LIVE_MARK_CAPACITY);
        size.values::<u32>(2);
        size.bytes
    }
}

/// Bytes of an automation scratch of `shape`, from [`scratch_shape`].
pub(crate) fn scratch_size(size: &mut Size, [words, resources, windows, points]: [usize; 4]) {
    size.values::<u32>(words);
    size.values::<ResourceSlot>(resources);
    size.values::<WindowState>(windows);
    size.values::<CurvePoint>(points);
    size.values::<CrossingSegment>(points);
    size.values::<f32>(points);
    size.values::<f32>(points);
}

pub(crate) fn carve_scratch<'w>(
    arena: &mut Carve<'w>,
    [words, resources, windows, points]: [usize; 4],
) -> Option<Scratch<'w>> {
    Some(Scratch {
        words: arena.values(words, 0)?,
        resources: arena.values(resources, ResourceSlot::default())?,
        windows: arena.values(windows, WindowState::default())?,
        points: arena.values(points, CurvePoint::default())?,
        crossings: arena.values(points, CrossingSegment::default())?,
        areas: arena.values(points, 0.0)?,
        slopes: arena.values(points, 0.0)?,
    })
}

fn frames<'w, const N: usize>(
    arena: &mut Carve<'w>,
    count: usize,
    pixels: usize,
) -> Option<[&'w mut [Color]; N]> {
    let mut frames: [&'w mut [Color]; N] = core::array::from_fn(|_| Default::default());
    for frame in frames.iter_mut().take(count) {
        *frame = arena.values(pixels, Color::BLACK)?;
    }
    Some(frames)
}

/// The workspace of `show` in `buffer`, which holds [`Layout::size`] bytes:
/// filled when `fresh`, or re-borrowed as an earlier carve of the same show
/// left it.
pub(crate) fn carve<'w>(
    show: Show<'_>,
    layout: &Layout,
    buffer: &'w mut [u8],
    fresh: bool,
) -> Option<Workspace<'w>> {
    let s = &show.s;
    let pixels = layout.pixels;
    let mut arena = if fresh {
        Carve::new(buffer)
    } else {
        Carve::again(buffer)
    };
    let buffers = frames(&mut arena, layout.buffers, pixels)?;
    let states = frames(&mut arena, layout.states, pixels)?;
    let scans = frames(&mut arena, layout.scans, pixels)?;
    let outputs = arena.values(layout.output_bytes, 0)?;
    let strip = layout.slots.carve(&mut arena)?;
    let scratch = carve_scratch(&mut arena, layout.scratch)?;
    let gather = arena.values(STRIP, [0, 0])?;
    let effect_samples = arena.values(layout.sample_count, CachedEffectSample::default())?;
    let timing = arena.values(layout.effects, Timing::default())?;
    let layer_order = arena.values(layout.layer_order, 0)?;
    let layer_ends = arena.values(layout.layer_order, 0)?;
    let overrides = arena.values(layout.bindings, Override::default())?;
    let live_tracks = arena.values(layout.collections, LiveTrack::default())?;
    let live_ticks = arena.values(layout.collections * LIVE_MARK_CAPACITY, 0)?;
    let frame = <&mut [u32; 2]>::try_from(arena.values(2, NONE)?).ok()?;
    if fresh {
        if layout.rig {
            for (timing, effect) in timing.iter_mut().zip(s.effects) {
                *timing = Timing {
                    start: effect.start,
                    duration: effect.duration,
                };
            }
            for voice in s.voices {
                timing[voice.effect as usize].start = DORMANT;
            }
            layer_order.copy_from_slice(s.layer_order);
            for layer in s.layers {
                crate::evaluation::index_layer(layer, timing, layer_order, layer_ends);
            }
        }
        for (index, track) in live_tracks.iter_mut().enumerate() {
            *track = LiveTrack {
                start: (index * LIVE_MARK_CAPACITY) as u32,
                len: 0,
            };
        }
    }
    Some(Workspace {
        buffers,
        states,
        scans,
        outputs,
        strip,
        scratch,
        gather,
        effect_samples,
        rig: RigState {
            timing,
            layer_order,
            layer_ends,
            overrides,
            live_tracks,
            live_ticks,
        },
        frame,
    })
}
