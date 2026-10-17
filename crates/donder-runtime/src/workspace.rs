//! A show's playback workspace, carved from one buffer: the frame and port
//! bytes, the strip workspaces, frame caches, automation and live state.
//! [`Layout`] counts every region once; [`Layout::size`] and [`carve`] walk
//! them in the same order.
use bytemuck::{Pod, Zeroable};
use donder_runtime_types::bytecode::{Direction, Instruction};
use donder_runtime_types::image::{self, CrossingSegment, CurvePoint, NONE};
use donder_runtime_types::{Color, SampleTime};

use crate::arena::{Carve, Size};
use crate::dsl::{STRIP, StateRecord, States, StripSlots, StripWorkspace, state_shape};
use crate::resources::{LiveTrack, ResourceSlot, WindowState};
use crate::show::Show;

/// The deepest operator nesting a show may have.
pub(crate) const MAX_VM_SLOTS: usize = 16;
/// The most marks a live collection keeps; older marks are dropped.
pub const LIVE_MARK_CAPACITY: usize = 1024;
/// The most whole-frame caches a show's operator depth slots hold together.
pub(crate) const MAX_FRAMES: usize = 64;
/// Pixels beyond the strip, on each side, that an addressed read's window
/// also evaluates, so nearby offsets reuse it.
pub(crate) const WINDOW_MARGIN: usize = 16;
/// Room for a strip, the strip shifted by up to a strip, and both margins.
pub(crate) const WINDOW: usize = 2 * STRIP + 2 * WINDOW_MARGIN;
/// A dormant voice starts at the end of time, so it is never active and
/// sorts after every started effect of its layer.
pub(crate) const DORMANT: u32 = u32::MAX;

/// What a cached frame or window holds: a node at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Key {
    pub(crate) node: u32,
    pub(crate) time: u32,
}

impl Key {
    pub(crate) const NONE: Self = Self {
        node: NONE,
        time: NONE,
    };

    pub(crate) fn of(node: usize, time: SampleTime) -> Self {
        Self {
            node: node as u32,
            time: time.as_ticks(),
        }
    }
}

/// One input's colors at one time over plan-target pixels `start..start + len`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WindowKey {
    pub(crate) key: Key,
    pub(crate) start: usize,
    pub(crate) len: usize,
}

impl WindowKey {
    pub(crate) const NONE: Self = Self {
        key: Key::NONE,
        start: 0,
        len: 0,
    };
}

#[derive(Default)]
pub(crate) struct CachedWindow<'w> {
    pub(crate) key: WindowKey,
    pub(crate) colors: &'w mut [Color],
}

/// An effect sample kept for duplicate fixture layouts.
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct CachedEffectSample {
    pub(crate) pixel_count: u32,
    pub(crate) color: Color,
    pub(crate) pad: u8,
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
        ticks >= self.start
            && self
                .start
                .checked_add(self.duration)
                .is_some_and(|end| ticks < end)
    }
}

/// A time-scan cache's state: the frame its running values hold, and where
/// they start in the running buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct TemporalRecord {
    pub(crate) frame: u32,
    pub(crate) start: u32,
}

/// What every operator and effect shares: the effect strip workspace, frame
/// caches, windows, effect automation, timing and live marks.
pub(crate) struct SamplingWorkspace<'w> {
    pub(crate) effect_strip: StripWorkspace<'w>,
    /// Every depth slot's whole-frame caches, slot after slot: a key and a
    /// frame each. A cache is taken out while it is computed.
    pub(crate) frame_keys: &'w mut [Key],
    pub(crate) frames: [&'w mut [Color]; MAX_FRAMES],
    /// Each depth slot's first cache.
    pub(crate) frame_base: [u16; MAX_VM_SLOTS],
    pub(crate) windows: [CachedWindow<'w>; MAX_VM_SLOTS],
    pub(crate) gather: &'w mut [[u32; 2]],
    pub(crate) effect_samples: &'w mut [CachedEffectSample],
    pub(crate) effect_states: States<'w>,
    /// A rig's effect timing and layer order, which its commands move; a
    /// sequence plays the image's, and these are empty.
    pub(crate) timing: &'w mut [Timing],
    pub(crate) layer_order: &'w mut [u32],
    pub(crate) layer_ends: &'w mut [u32],
    /// Each node's operator automation state, or [`NONE`].
    pub(crate) operator_state: &'w [u32],
    /// Each node's first time-scan record, or [`NONE`]; its frame caches
    /// index from there.
    pub(crate) temporal_slot: &'w [u32],
    pub(crate) live_tracks: &'w mut [LiveTrack],
    pub(crate) live_ticks: &'w mut [u32],
}

pub(crate) struct Workspace<'w> {
    pub(crate) signal_buffer: &'w mut [Color],
    pub(crate) outputs: &'w mut [u8],
    pub(crate) operator_states: States<'w>,
    pub(crate) strips: [Option<StripWorkspace<'w>>; MAX_VM_SLOTS],
    /// Time scans' records and running values, in node order.
    pub(crate) temporal: &'w mut [TemporalRecord],
    pub(crate) running: &'w mut [[f32; 3]],
    pub(crate) sampling: SamplingWorkspace<'w>,
}

impl Workspace<'_> {
    /// Drop every cached sample, for a show whose parameters or effect
    /// windows changed without the time changing.
    pub(crate) fn forget_samples(&mut self) {
        self.operator_states.forget();
        self.sampling.effect_states.forget();
        for record in self.temporal.iter_mut() {
            record.frame = NONE;
        }
        self.sampling.frame_keys.fill(Key::NONE);
        for window in &mut self.sampling.windows {
            window.key = WindowKey::NONE;
        }
    }
}

/// The counts of a state set: records, then words, resources, windows,
/// points and overrides.
#[derive(Clone, Copy, Debug, Default)]
struct StateCounts {
    records: usize,
    parts: [usize; 5],
}

/// A show's playback buffer: every region's count, derived from the show once.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pixels: usize,
    output_bytes: usize,
    nodes: usize,
    effects: usize,
    layer_order: usize,
    collections: usize,
    vm_slots: usize,
    effect_slots: StripSlots,
    slots: [StripSlots; MAX_VM_SLOTS],
    frames: [usize; MAX_VM_SLOTS],
    /// Whether commands move effects: the show has voices.
    rig: bool,
    sample_count: usize,
    operator_states: StateCounts,
    effect_states: StateCounts,
    temporal: usize,
    running: usize,
}

/// Whether the effect needs a state: automation, or a live marks parameter.
fn effect_invocations(show: Show<'_>) -> impl Iterator<Item = (u32, u32)> + '_ {
    show.s
        .effects
        .iter()
        .filter(|effect| effect.state != NONE)
        .map(|effect| (effect.automation, effect.params))
}

fn operator_invocations(show: Show<'_>) -> impl Iterator<Item = (u32, u32)> + '_ {
    show.s
        .nodes
        .iter()
        .filter(|node| node.kind == image::NODE_OPERATOR && node.automation != NONE)
        .map(|node| (node.automation, node.params))
}

fn state_counts(show: Show<'_>, invocations: impl Iterator<Item = (u32, u32)>) -> StateCounts {
    let mut counts = StateCounts::default();
    for (plan, params) in invocations {
        let (words, resources, windows, points, overrides) = state_shape(show, plan, params);
        counts.records += 1;
        for (total, add) in counts
            .parts
            .iter_mut()
            .zip([words, resources, windows, points, overrides])
        {
            *total += add;
        }
    }
    counts
}

/// Whether frame cache `cache` of `node`'s program holds a time scan.
fn is_time_scan(show: Show<'_>, node: &image::Node, cache: usize) -> bool {
    show.program(node.program).code.iter().any(|word| {
        matches!(
            word.decode_trusted(),
            Some(Instruction::Scan {
                direction: Direction::Time,
                cache: scan,
                ..
            }) if usize::from(scan) == cache
        )
    })
}

/// The records and time scans of `node`: a record per frame cache when it
/// has any time scan.
fn temporal_caches(show: Show<'_>, node: &image::Node) -> (usize, usize) {
    if node.kind != image::NODE_OPERATOR {
        return (0, 0);
    }
    let caches = show.program(node.program).frame_cache_count();
    let scans = (0..caches)
        .filter(|&cache| is_time_scan(show, node, cache))
        .count();
    if scans == 0 { (0, 0) } else { (caches, scans) }
}

impl Layout {
    pub(crate) fn of(show: Show<'_>) -> Self {
        let s = &show.s;
        let mut effect_slots = StripSlots::default();
        for effect in s.effects {
            effect_slots.include(show.program(effect.program));
        }
        let mut slots = [StripSlots::default(); MAX_VM_SLOTS];
        let mut frames = [0; MAX_VM_SLOTS];
        let (mut temporal, mut scans) = (0, 0);
        for node in s.nodes {
            if node.kind == image::NODE_OPERATOR {
                let program = show.program(node.program);
                let slot = node.vm_slot as usize;
                slots[slot].include(program);
                frames[slot] = frames[slot].max(program.frame_cache_count());
                let (records, count) = temporal_caches(show, node);
                temporal += records;
                scans += count;
            }
        }
        let pixels = show.pixel_count();
        Self {
            pixels,
            output_bytes: s.outputs.iter().map(|output| output.width as usize).sum(),
            nodes: s.nodes.len(),
            effects: s.effects.len(),
            layer_order: s.layer_order.len(),
            collections: s.collections.len(),
            vm_slots: show.meta().vm_slots as usize,
            effect_slots,
            slots,
            frames,
            rig: !s.voices.is_empty(),
            sample_count: s
                .effects
                .iter()
                .map(|effect| s.targets[effect.target as usize].sample_count as usize)
                .max()
                .unwrap_or(0),
            operator_states: state_counts(show, operator_invocations(show)),
            effect_states: state_counts(show, effect_invocations(show)),
            temporal,
            running: scans * pixels,
        }
    }

    /// The effects and layer order entries a rig's overlays hold.
    fn overlays(&self) -> (usize, usize) {
        if self.rig {
            (self.effects, self.layer_order)
        } else {
            (0, 0)
        }
    }

    /// The whole-frame caches of every depth slot together.
    pub(crate) fn frame_caches(&self) -> usize {
        self.frames.iter().sum()
    }

    /// Words of the buffer a playback of the show needs.
    pub fn words(&self) -> usize {
        self.size().div_ceil(8)
    }

    /// Bytes of the buffer [`carve`] needs.
    pub(crate) fn size(&self) -> usize {
        let mut size = Size::default();
        size.values::<Color>(self.pixels);
        size.values::<u8>(self.output_bytes);
        size_states(&mut size, self.operator_states);
        for slot in &self.slots[..self.vm_slots] {
            slot.size(&mut size);
        }
        size.values::<TemporalRecord>(self.temporal);
        size.values::<[f32; 3]>(self.running);
        self.effect_slots.size(&mut size);
        let caches: usize = self.frames.iter().sum();
        size.values::<Key>(caches);
        for _ in 0..caches {
            size.values::<Color>(self.pixels);
        }
        for _ in 0..self.vm_slots {
            size.values::<Color>(WINDOW);
        }
        size.values::<[u32; 2]>(STRIP);
        size.values::<CachedEffectSample>(self.sample_count);
        size_states(&mut size, self.effect_states);
        let (effects, order) = self.overlays();
        size.values::<Timing>(effects);
        size.values::<u32>(order);
        size.values::<u32>(order);
        size.values::<u32>(self.nodes);
        size.values::<u32>(self.nodes);
        size.values::<LiveTrack>(self.collections);
        size.values::<u32>(self.collections * LIVE_MARK_CAPACITY);
        size.bytes
    }
}

fn size_states(size: &mut Size, counts: StateCounts) {
    let [words, resources, windows, points, overrides] = counts.parts;
    size.values::<StateRecord>(counts.records);
    size.values::<u32>(words);
    size.values::<ResourceSlot>(resources);
    size.values::<WindowState>(windows);
    size.values::<CurvePoint>(points);
    size.values::<f32>(points);
    size.values::<CrossingSegment>(points);
    size.values::<crate::dsl::Override>(overrides);
}

fn carve_states<'w>(
    arena: &mut Carve<'w>,
    show: Show<'_>,
    counts: StateCounts,
    invocations: impl Iterator<Item = (u32, u32)>,
) -> Option<States<'w>> {
    let [words, resources, windows, points, overrides] = counts.parts;
    let mut states = States {
        records: arena.values(counts.records, StateRecord::default())?,
        words: arena.values(words, 0)?,
        resources: arena.values(resources, ResourceSlot::default())?,
        windows: arena.values(windows, WindowState::default())?,
        points: arena.values(points, CurvePoint::default())?,
        areas: arena.values(points, 0.0)?,
        crossings: arena.values(points, CrossingSegment::default())?,
        overrides: arena.values(overrides, crate::dsl::Override::default())?,
    };
    if arena.fills() {
        let mut at = [0u32; 5];
        for (record, (plan, params)) in states.records.iter_mut().zip(invocations) {
            let (words, resources, windows, points, overrides) = state_shape(show, plan, params);
            let mut range = |part: usize, len: usize| {
                let range = image::Range {
                    start: at[part],
                    len: len as u32,
                };
                at[part] += len as u32;
                range
            };
            *record = StateRecord {
                plan,
                params,
                words: range(0, words),
                resources: range(1, resources),
                windows: range(2, windows),
                points: range(3, points),
                overrides: range(4, overrides),
                origin: show.plan(plan).map_or(0, |plan| plan.origin),
                time: NONE,
            };
        }
        for index in 0..states.records.len() {
            states.get(index).detach(show);
        }
    }
    Some(states)
}

/// Number the items `needs` accepts, in order, into `slots`; others hold
/// [`NONE`].
fn number(slots: &mut [u32], needs: impl Fn(usize) -> bool) {
    let mut next = 0;
    for (index, slot) in slots.iter_mut().enumerate() {
        if needs(index) {
            *slot = next;
            next += 1;
        }
    }
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
    let signal_buffer = arena.values(pixels, Color::BLACK)?;
    let outputs = arena.values(layout.output_bytes, 0)?;
    let operator_states = carve_states(
        &mut arena,
        show,
        layout.operator_states,
        operator_invocations(show),
    )?;
    let mut strips: [Option<StripWorkspace<'w>>; MAX_VM_SLOTS] = Default::default();
    for (strip, slot) in strips.iter_mut().zip(&layout.slots[..layout.vm_slots]) {
        *strip = Some(slot.carve(&mut arena)?);
    }
    let temporal = arena.values(layout.temporal, TemporalRecord::default())?;
    let running = arena.values(layout.running, [0.0; 3])?;
    let effect_strip = layout.effect_slots.carve(&mut arena)?;
    let frame_keys = arena.values(layout.frame_caches(), Key::NONE)?;
    let mut frames: [&'w mut [Color]; MAX_FRAMES] = core::array::from_fn(|_| Default::default());
    for frame in frames.iter_mut().take(layout.frame_caches()) {
        *frame = arena.values(pixels, Color::BLACK)?;
    }
    let mut frame_base = [0u16; MAX_VM_SLOTS];
    let mut base = 0;
    for (first, &count) in frame_base.iter_mut().zip(&layout.frames) {
        *first = base as u16;
        base += count;
    }
    let mut windows: [CachedWindow<'w>; MAX_VM_SLOTS] = Default::default();
    for window in windows.iter_mut().take(layout.vm_slots) {
        *window = CachedWindow {
            key: WindowKey::NONE,
            colors: arena.values(WINDOW, Color::BLACK)?,
        };
    }
    let gather = arena.values(STRIP, [0, 0])?;
    let effect_samples = arena.values(layout.sample_count, CachedEffectSample::default())?;
    let effect_states = carve_states(
        &mut arena,
        show,
        layout.effect_states,
        effect_invocations(show),
    )?;
    let (effects, order) = layout.overlays();
    let timing = arena.values(effects, Timing::default())?;
    let layer_order = arena.values(order, 0)?;
    let layer_ends = arena.values(order, 0)?;
    if fresh && layout.rig {
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
    let operator_state = arena.values(layout.nodes, NONE)?;
    let temporal_slot = arena.values(layout.nodes, NONE)?;
    if fresh {
        number(operator_state, |index| {
            let node = &s.nodes[index];
            node.kind == image::NODE_OPERATOR && node.automation != NONE
        });
        let (mut next, mut start) = (0, 0);
        for (slot, node) in temporal_slot.iter_mut().zip(s.nodes) {
            let (records, _) = temporal_caches(show, node);
            if records == 0 {
                continue;
            }
            *slot = next as u32;
            for cache in 0..records {
                temporal[next + cache] = TemporalRecord {
                    frame: NONE,
                    start: start as u32,
                };
                if is_time_scan(show, node, cache) {
                    start += pixels;
                }
            }
            next += records;
        }
    }
    let live_tracks = arena.values(layout.collections, LiveTrack::default())?;
    let live_ticks = arena.values(layout.collections * LIVE_MARK_CAPACITY, 0)?;
    if fresh {
        for (index, track) in live_tracks.iter_mut().enumerate() {
            *track = LiveTrack {
                start: (index * LIVE_MARK_CAPACITY) as u32,
                len: 0,
            };
        }
    }
    Some(Workspace {
        signal_buffer,
        outputs,
        operator_states,
        strips,
        temporal,
        running,
        sampling: SamplingWorkspace {
            effect_strip,
            frame_keys,
            frames,
            frame_base,
            windows,
            gather,
            effect_samples,
            effect_states,
            timing,
            layer_order,
            layer_ends,
            operator_state,
            temporal_slot,
            live_tracks,
            live_ticks,
        },
    })
}

#[cfg(feature = "host")]
/// Bytes of one effect's own automation state, as a clip sampler keeps it.
pub(crate) fn effect_state_size(show: Show<'_>, effect: usize, size: &mut Size) {
    size_states(size, state_counts(show, own_invocation(show, effect)));
}

#[cfg(feature = "host")]
/// One effect's own automation state; empty without automation.
pub(crate) fn carve_effect_state<'w>(
    arena: &mut Carve<'w>,
    show: Show<'_>,
    effect: usize,
) -> Option<States<'w>> {
    let counts = state_counts(show, own_invocation(show, effect));
    carve_states(arena, show, counts, own_invocation(show, effect))
}

#[cfg(feature = "host")]
fn own_invocation(show: Show<'_>, effect: usize) -> impl Iterator<Item = (u32, u32)> {
    let effect = &show.s.effects[effect];
    (effect.automation != NONE)
        .then_some((effect.automation, effect.params))
        .into_iter()
}
