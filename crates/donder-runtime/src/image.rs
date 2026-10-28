//! The prepared sequence's format: what preparation writes and playback
//! borrows. It is a header and a table of sections; each section is an array
//! of fixed-size records, and records refer to one another by index. Playback
//! reads the records in place, without decoding them.
//!
//! Every record is `#[repr(C)]` plain data of 32-bit fields (16-bit where a
//! word is made of them), so its bytes are the same on every little-endian
//! target. The format is trusted: preparation wrote it, so playback checks
//! only its version.
use bytemuck::{Pod, Zeroable};

/// No index: an absent optional reference.
pub(crate) const NONE: u32 = u32::MAX;
/// Every section starts on this alignment within the payload.
pub(crate) const ALIGN: usize = 4;

/// One section's place in the payload: a byte offset and a record count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct SectionEntry {
    pub(crate) offset: u32,
    pub(crate) count: u32,
}

/// The fixed part of an image header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Header {
    /// The format version; an image of another version is not read.
    pub(crate) version: u32,
    /// Payload bytes after the header and section table.
    pub(crate) payload: u32,
    /// The image's identity, a hash of its table and payload: a controller
    /// reports it so a host can tell which show it holds. Playback never
    /// reads it.
    pub(crate) id: u32,
    pub(crate) reserved: u32,
}

/// Bytes of a header with its section table.
pub const fn header_bytes() -> usize {
    size_of::<Header>() + SECTION_COUNT * size_of::<SectionEntry>()
}

/// A half-open range of records in another section.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Range {
    pub(crate) start: u32,
    pub(crate) len: u32,
}

impl Range {
    pub(crate) const EMPTY: Self = Self { start: 0, len: 0 };

    pub(crate) fn get<T>(self, items: &[T]) -> &[T] {
        let start = self.start as usize;
        &items[start..start + self.len as usize]
    }
}

/// Sequence-wide values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Meta {
    pub(crate) frame_rate: u32,
    pub(crate) frame_count: u32,
    /// Ticks.
    pub(crate) duration: u32,
    pub(crate) pixel_count: u32,
    /// The target every frame and operator covers: every retained pixel.
    pub(crate) plan_target: u32,
    /// The node the output shows, or [`NONE`] for black.
    pub(crate) output: u32,
    /// Frame buffers the nodes write, each one plan-target frame.
    pub(crate) buffers: u32,
    /// State buffers: the previous frames of the nodes that read them.
    pub(crate) states: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct Fixture {
    pub id: u32,
    pub pixel_count: u32,
    /// The fixture's first pixel in the plan target.
    pub offset: u32,
    /// Layout positions of its retained cells, for spatial programs; empty
    /// when no program reads them.
    pub(crate) positions: Range,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Position {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

/// How a target lists its pixels.
pub(crate) const PIXELS_RUNS: u32 = 0;
pub(crate) const PIXELS_INDEXED: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Target {
    /// [`PIXELS_RUNS`] into the runs section or [`PIXELS_INDEXED`] into the
    /// pixels section.
    pub(crate) kind: u32,
    pub(crate) pixels: Range,
    pub(crate) spatial: Range,
    pub(crate) section_runs: Range,
    /// [`PIXELS_RUNS`] into section spans or [`PIXELS_INDEXED`] into
    /// section pixels; empty without section queries.
    pub(crate) section_kind: u32,
    pub(crate) section_pixels: Range,
    /// Nonzero when sections count within each fixture.
    pub(crate) per_fixture: u32,
    /// Distinct logical pixels shared by duplicate fixture layouts, or zero.
    pub(crate) sample_count: u32,
}

/// Consecutive target pixels with one fixture, count and contiguous cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct PixelRun {
    /// One past the run's last target index.
    pub(crate) end: u32,
    pub(crate) fixture: u32,
    pub(crate) first_cell: u32,
    pub(crate) first_index: u32,
    pub(crate) count: u32,
}

/// One target pixel: its storage fixture and cell, and its logical index
/// within a scope of `count` pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Pixel {
    pub(crate) fixture: u32,
    pub(crate) cell: u32,
    pub(crate) index: u32,
    pub(crate) count: u32,
}

/// Consecutive pixels sharing their scope's bounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct SpatialRun {
    pub(crate) end: u32,
    pub(crate) min: [f32; 2],
    pub(crate) max: [f32; 2],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct SectionRun {
    pub(crate) fixture: u32,
    pub(crate) first: u32,
    pub(crate) last: u32,
}

/// A pixel's place among a target's section runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct SectionPixel {
    pub(crate) run: u32,
    pub(crate) index: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct SectionSpan {
    pub(crate) end: u32,
    pub(crate) first: SectionPixel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Effect {
    /// Ticks.
    pub(crate) start: u32,
    pub(crate) duration: u32,
    pub(crate) target: u32,
    pub(crate) program: u32,
    pub(crate) params: u32,
    /// Its automation plan, or [`NONE`].
    pub(crate) automation: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Clip {
    pub(crate) id: u32,
    pub(crate) effect: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Layer {
    pub(crate) enabled: u32,
    /// Its effects, sorted by start, in the layer order section; the same
    /// range of the layer ends section holds, for each, the latest end of it
    /// and every effect before it.
    pub(crate) effects: Range,
}

pub(crate) const NODE_LAYER: u32 = 0;
pub(crate) const NODE_OPERATOR: u32 = 1;
pub(crate) const NODE_MIX: u32 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Node {
    pub(crate) kind: u32,
    /// The layer of a layer node.
    pub(crate) layer: u32,
    pub(crate) program: u32,
    pub(crate) params: u32,
    /// Input nodes, in the node inputs section.
    pub(crate) inputs: Range,
    /// Its automation plan, or [`NONE`].
    pub(crate) automation: u32,
    /// The frame buffer it writes.
    pub(crate) buffer: u32,
    /// Its state buffer, which holds its previous frame, or [`NONE`]. A
    /// layer's clips read it as `previous`, as an operator does its own.
    pub(crate) state: u32,
}

/// Program flags.
pub(crate) const PROGRAM_USES_SPATIAL: u32 = 1 << 2;
pub(crate) const PROGRAM_USES_SECTIONS: u32 = 1 << 3;
pub(crate) const PROGRAM_READS_TARGET: u32 = 1 << 4;
pub(crate) const PROGRAM_USES_PIXEL_CONTEXT: u32 = 1 << 5;
pub(crate) const PROGRAM_HAS_STENCIL: u32 = 1 << 6;
/// The program reads `previous`.
pub(crate) const PROGRAM_READS_STATE: u32 = 1 << 7;

/// Slot counts per storage bank: floats, ints, bools, resources.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct BankCounts {
    pub(crate) counts: [u16; 4],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Program {
    pub(crate) code: Range,
    pub(crate) query_end: u32,
    pub(crate) target_end: u32,
    pub(crate) result: u32,
    pub(crate) depth: u32,
    pub(crate) scalars: BankCounts,
    pub(crate) rows: BankCounts,
    /// Constant resources, as indices into their kinds' sections, in the
    /// program resources section.
    pub(crate) resources: Range,
    /// Whole-frame scans, each with a scratch frame while the node runs.
    pub(crate) scans: u32,
    /// An operator's input count; zero for an effect.
    pub(crate) inputs: u32,
    pub(crate) flags: u32,
}

/// Bound parameter values: words, and resource references.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct ParamSet {
    pub(crate) words: Range,
    pub(crate) resources: Range,
}

pub(crate) const RESOURCE_CURVE: u32 = 0;
pub(crate) const RESOURCE_GRADIENT: u32 = 1;
pub(crate) const RESOURCE_MARKS: u32 = 2;
pub(crate) const RESOURCE_ARRAY: u32 = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct ResourceRef {
    pub(crate) kind: u32,
    pub(crate) index: u32,
}

/// Whether a curve's segments only rise, only fall, or both.
pub(crate) const CROSSINGS_INCREASING: u32 = 0;
pub(crate) const CROSSINGS_DECREASING: u32 = 1;
pub(crate) const CROSSINGS_MIXED: u32 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Curve {
    pub(crate) points: Range,
    /// Each point's area from the first, in the curve areas section.
    pub(crate) areas: Range,
    /// Each point's slope from the point before it, in the curve slopes
    /// section.
    pub(crate) slopes: Range,
    pub(crate) crossings_kind: u32,
    pub(crate) crossings: Range,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct CrossingSegment {
    pub(crate) position_bias: f32,
    pub(crate) position_scale: f32,
    pub(crate) min_value: f32,
    pub(crate) max_value: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Gradient {
    pub(crate) stops: Range,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct GradientStop {
    pub(crate) position: f32,
    pub(crate) color: Color,
    /// One over the width of the span to the next stop; zero for the last.
    pub(crate) inverse: f32,
}

/// A window of a mark track: ticks `first..end` of the track, measured from
/// `origin`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Marks {
    pub(crate) track: Range,
    pub(crate) first: u32,
    pub(crate) end: u32,
    pub(crate) origin: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Array {
    pub(crate) items: Range,
}

pub(crate) const VALUE_VOID: u32 = 0;
pub(crate) const VALUE_INT: u32 = 1;
pub(crate) const VALUE_FLOAT: u32 = 2;
pub(crate) const VALUE_BOOL: u32 = 3;
pub(crate) const VALUE_COLOR: u32 = 4;
/// An enum option's index in its program's names.
pub(crate) const VALUE_ENUM: u32 = 5;
pub(crate) const VALUE_MARKS: u32 = 6;
pub(crate) const VALUE_CURVE: u32 = 7;
pub(crate) const VALUE_GRADIENT: u32 = 8;
pub(crate) const VALUE_ARRAY: u32 = 9;

/// An array item: its kind and a word: a scalar's bits, or for a color or
/// a resource its index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Value {
    pub(crate) kind: u32,
    pub(crate) word: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Automation {
    pub(crate) bindings: Range,
    pub(crate) windows: Range,
}

/// What an automation binding writes.
pub(crate) const BINDING_FLOAT: u32 = 0;
pub(crate) const BINDING_FLOAT_INTEGRAL: u32 = 1;
pub(crate) const BINDING_INT: u32 = 2;
pub(crate) const BINDING_BOOL: u32 = 3;
pub(crate) const BINDING_ENUM: u32 = 4;
pub(crate) const BINDING_CURVE: u32 = 5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Binding {
    pub(crate) parameter: u32,
    pub(crate) start: u32,
    pub(crate) duration: u32,
    pub(crate) curve: u32,
    pub(crate) kind: u32,
    /// The word it writes, or for a curve its window.
    pub(crate) destination: u32,
    /// Float or int bounds, as bits.
    pub(crate) min: u32,
    pub(crate) max: u32,
    /// An enum's option indices, in the enum options section.
    pub(crate) options: Range,
}

/// A curve resource that bindings rewrite, and the most points they write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Window {
    pub(crate) resource: u32,
    pub(crate) points: u32,
}

pub(crate) const ENCODING_RGB: u32 = 0;
pub(crate) const ENCODING_RGBW: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Route {
    /// Plan-target pixels `first..end`.
    pub(crate) first: u32,
    pub(crate) end: u32,
    pub(crate) output: u32,
    pub(crate) start_slot: u32,
    pub(crate) encoding: u32,
    pub(crate) order: [u8; 4],
    /// Its lookup table, or [`NONE`].
    pub(crate) lookup: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Lookup {
    pub(crate) table: [u8; 256],
}

impl Default for Lookup {
    fn default() -> Self {
        Self { table: [0; 256] }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct Output {
    pub controller_index: u32,
    pub port: u32,
    /// Where its bytes start in the frame's output bytes, after every
    /// output before it.
    pub offset: u32,
    /// Bytes.
    pub width: u32,
}

/// A voice of length [`NONE`] plays until stopped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Voice {
    pub(crate) effect: u32,
    pub(crate) duration: u32,
}

/// A live mark collection: the marks parameters reading it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Collection {
    pub(crate) readers: Range,
}

/// An effect's parameter that reads a live collection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Reader {
    pub(crate) effect: u32,
    /// Its resource bank.
    pub(crate) bank: u32,
}

/// A control: the automation bindings it sets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Control {
    pub(crate) targets: Range,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct ControlTarget {
    pub(crate) effect: u32,
    pub(crate) binding: u32,
}

pub(crate) use donder_runtime_types::InstructionWord;
pub(crate) use donder_runtime_types::{Color, CurvePoint};

macro_rules! sections {
    ($($name:ident: $ty:ty,)+) => {
        /// Every section of an image, borrowed from its payload.
        #[derive(Clone, Copy, Debug)]
        pub(crate) struct Sections<'a> {
            $(pub(crate) $name: &'a [$ty],)+
        }

        pub(crate) const SECTION_COUNT: usize = [$(stringify!($name)),+].len();

        impl<'a> Sections<'a> {
            /// Each section of `table`, borrowed from `payload`.
            fn borrow(payload: &'a [u8], table: &[SectionEntry]) -> Self {
                let mut at = 0;
                let mut next = || {
                    at += 1;
                    table[at - 1]
                };
                Self {
                    $($name: section::<$ty>(payload, next()),)+
                }
            }
        }

        /// Sections being written: each a growing list of records.
        #[cfg(feature = "host")]
        #[derive(Clone, Debug, Default)]
        pub(crate) struct SectionsWriter {
            $(pub(crate) $name: alloc::vec::Vec<$ty>,)+
        }

        #[cfg(feature = "host")]
        impl SectionsWriter {
            /// The image: its header, section table and payload, each section
            /// aligned to [`ALIGN`] bytes. The buffer is 4-byte aligned.
            pub(crate) fn finish(&self, version: u32) -> alloc::boxed::Box<[u32]> {
                let mut table = alloc::vec::Vec::new();
                let mut payload = alloc::vec::Vec::<u8>::new();
                $(
                    while payload.len() % ALIGN != 0 {
                        payload.push(0);
                    }
                    table.push(SectionEntry {
                        offset: payload.len() as u32,
                        count: self.$name.len() as u32,
                    });
                    payload.extend_from_slice(bytemuck::cast_slice(&self.$name));
                )+
                while payload.len() % ALIGN != 0 {
                    payload.push(0);
                }
                frame(version, &table, &payload)
            }
        }
    };
}

sections! {
    meta: Meta,
    fixtures: Fixture,
    positions: Position,
    targets: Target,
    pixel_runs: PixelRun,
    pixels: Pixel,
    spatial_runs: SpatialRun,
    section_runs: SectionRun,
    section_spans: SectionSpan,
    section_pixels: SectionPixel,
    effects: Effect,
    clips: Clip,
    layers: Layer,
    layer_order: u32,
    layer_ends: u32,
    nodes: Node,
    node_inputs: u32,
    programs: Program,
    code: InstructionWord,
    program_resources: u32,
    param_sets: ParamSet,
    words: u32,
    colors: Color,
    resources: ResourceRef,
    curves: Curve,
    curve_points: CurvePoint,
    curve_areas: f32,
    curve_slopes: f32,
    crossings: CrossingSegment,
    gradients: Gradient,
    gradient_stops: GradientStop,
    marks: Marks,
    tracks: u32,
    arrays: Array,
    values: Value,
    automation: Automation,
    bindings: Binding,
    enum_options: u32,
    windows: Window,
    routes: Route,
    lookups: Lookup,
    outputs: Output,
    voices: Voice,
    collections: Collection,
    readers: Reader,
    controls: Control,
    control_targets: ControlTarget,
}

fn section<T: Pod>(payload: &[u8], entry: SectionEntry) -> &[T] {
    let start = entry.offset as usize;
    bytemuck::cast_slice(&payload[start..start + entry.count as usize * size_of::<T>()])
}

/// A header, section table and payload in one 4-byte-aligned buffer.
#[cfg(feature = "host")]
fn frame(version: u32, table: &[SectionEntry], payload: &[u8]) -> alloc::boxed::Box<[u32]> {
    let table_bytes: &[u8] = bytemuck::cast_slice(table);
    // FNV-1a.
    let id = table_bytes
        .iter()
        .chain(payload)
        .fold(0x811c_9dc5u32, |hash, &byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
        });
    let header = Header {
        version,
        payload: payload.len() as u32,
        id,
        reserved: 0,
    };
    let mut bytes = alloc::vec::Vec::with_capacity(header_bytes() + payload.len());
    bytes.extend_from_slice(bytemuck::bytes_of(&header));
    bytes.extend_from_slice(table_bytes);
    bytes.extend_from_slice(payload);
    let mut words = alloc::vec![0u32; bytes.len().div_ceil(4)];
    bytemuck::cast_slice_mut::<u32, u8>(&mut words)[..bytes.len()].copy_from_slice(&bytes);
    words.into_boxed_slice()
}

/// The image's length from its header: the header, section table and
/// payload. Lets a receiver size the rest of a transfer.
pub(crate) fn image_len(header: &[u8]) -> usize {
    let header: Header = bytemuck::pod_read_unaligned(&header[..size_of::<Header>()]);
    header_bytes() + header.payload as usize
}

/// The image's sections, or `None` for an image of another format version.
/// The image is trusted otherwise: only its producer writes it. `bytes` must
/// be 4-byte aligned, as the writer's buffers and flash mappings are.
pub(crate) fn open(bytes: &[u8], version: u32) -> Option<Sections<'_>> {
    let header: Header = bytemuck::pod_read_unaligned(&bytes[..size_of::<Header>()]);
    if header.version != version {
        return None;
    }
    let table = bytemuck::cast_slice(&bytes[size_of::<Header>()..header_bytes()]);
    Some(Sections::borrow(&bytes[header_bytes()..], table))
}

impl donder_runtime_types::sampling::Stop for GradientStop {
    #[inline(always)]
    fn position(&self) -> f32 {
        self.position
    }
    #[inline(always)]
    fn color(&self) -> Color {
        self.color
    }
    #[inline(always)]
    fn inverse(&self, _: &Self) -> f32 {
        self.inverse
    }
}
