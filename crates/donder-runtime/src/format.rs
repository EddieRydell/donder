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
use donder_runtime_types::GradientSpace;

/// No index: an absent optional reference.
pub(crate) const NONE: u32 = u32::MAX;
/// Every section starts on this alignment within the payload.
const ALIGN: usize = 4;

/// One section's place in the payload: a byte offset and a record count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct SectionEntry {
    offset: u32,
    count: u32,
}

/// The fixed part of a show header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Header {
    /// The format version; a show of another version is not read.
    version: u32,
    /// Payload bytes after the header and section table.
    payload: u32,
    /// The show's identity, a hash of its table and payload: a controller
    /// reports it so a host can tell which show it holds. Playback never
    /// reads it.
    id: u32,
}

/// Bytes of a header with its section table.
const fn header_bytes() -> usize {
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
    /// The root node, whose frame the show plays, or [`NONE`] for black.
    pub(crate) root: u32,
    /// Frame buffers the nodes write, each one plan-target frame.
    pub(crate) buffers: u32,
    /// A console's live mark collections.
    pub(crate) collections: u32,
    /// The controls a console reads.
    pub(crate) controls: u32,
    /// Words of a console's live store: three per live automation, then one
    /// per snapshot.
    pub(crate) live_words: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct Fixture {
    pub id: u32,
    pub pixel_count: u32,
    /// The fixture's first pixel in the plan target.
    pub(crate) offset: u32,
    /// Layout positions of its retained cells, for spatial programs; empty
    /// when no program reads them.
    pub(crate) positions: Range,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Position {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) z: f32,
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
    /// Distinct logical pixels shared by duplicate fixture layouts, or zero.
    pub(crate) sample_count: u32,
    /// Each target pixel's place in the target's structure, for programs
    /// that read it; empty otherwise.
    pub(crate) structure: Range,
}

/// A target pixel's member, strand and place along the strand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct PixelStructure {
    pub(crate) member: u32,
    pub(crate) strand: u32,
    pub(crate) along: u32,
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

/// Consecutive pixels sharing their scope's bounds and structure counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct SpatialRun {
    pub(crate) end: u32,
    pub(crate) min: [f32; 3],
    pub(crate) max: [f32; 3],
    pub(crate) members: u32,
    pub(crate) strands: u32,
    pub(crate) strand_length: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Clip {
    /// Ticks.
    pub(crate) start: u32,
    pub(crate) duration: u32,
    pub(crate) target: u32,
    pub(crate) program: u32,
    pub(crate) params: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct ClipIndex {
    pub(crate) id: u32,
    pub(crate) clip: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Layer {
    pub(crate) enabled: u32,
    /// Its clips, sorted by start, in the layer order section; the same
    /// range of the layer ends section holds, for each, the latest end of it
    /// and every clip before it.
    pub(crate) clips: Range,
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
    /// The frame buffer it writes.
    pub(crate) buffer: u32,
    /// An operator's frame while every input is black, as an index in the
    /// colors section, or [`NONE`] when it must run.
    pub(crate) black_inputs: u32,
}

/// Program flags.
pub(crate) const PROGRAM_USES_SPATIAL: u32 = 1 << 0;
pub(crate) const PROGRAM_READS_TARGET: u32 = 1 << 1;
pub(crate) const PROGRAM_USES_PIXEL_CONTEXT: u32 = 1 << 2;
pub(crate) const PROGRAM_HAS_STENCIL: u32 = 1 << 3;
pub(crate) const PROGRAM_USES_STRUCTURE: u32 = 1 << 4;

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
    /// An operator's input count; zero for an effect's program.
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Gradient {
    /// The stops' colors in the coordinates of `space`.
    pub(crate) stops: Range,
    pub(crate) space: u32,
}

pub(crate) const SPACE_LINEAR_RGB: u32 = 0;
pub(crate) const SPACE_HSV: u32 = 1;
pub(crate) const SPACE_OKLAB: u32 = 2;

impl Gradient {
    pub(crate) fn encode_space(space: GradientSpace) -> u32 {
        match space {
            GradientSpace::LinearRgb => SPACE_LINEAR_RGB,
            GradientSpace::Hsv => SPACE_HSV,
            GradientSpace::Oklab => SPACE_OKLAB,
        }
    }

    #[inline(always)]
    pub(crate) fn space(&self) -> GradientSpace {
        match self.space {
            SPACE_LINEAR_RGB => GradientSpace::LinearRgb,
            SPACE_HSV => GradientSpace::Hsv,
            SPACE_OKLAB => GradientSpace::Oklab,
            _ => unreachable!("shows hold known gradient spaces"),
        }
    }
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
/// `origin`, each lasting the ticks `durations` holds at its index; empty
/// durations make every mark instantaneous.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Marks {
    pub(crate) track: Range,
    pub(crate) durations: Range,
    /// Label character codes, and each mark's start and length in them, by
    /// the track's index; empty spans without labels.
    pub(crate) label_codes: Range,
    pub(crate) label_spans: Range,
    pub(crate) first: u32,
    pub(crate) end: u32,
    pub(crate) origin: u32,
    /// The console's live collection read in its place, measured from the
    /// invocation's start, or [`NONE`].
    pub(crate) live: u32,
}

/// An audio track as a binding reads it: `channels` bytes per sample in the
/// audio samples section, `rate` samples a second, read from tick `origin`;
/// bytes are 0..1, or -1..1 when `signed` is 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct AudioTrack {
    pub(crate) samples: Range,
    pub(crate) channels: u32,
    pub(crate) signed: u32,
    pub(crate) rate: u32,
    pub(crate) origin: u32,
}

/// A font: its line height, its glyphs in the glyphs section and their
/// coverage bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Font {
    pub(crate) height: f32,
    pub(crate) glyphs: Range,
    pub(crate) coverage: Range,
}

/// An image: its size, its frames and their rate, and its pixels' bytes in
/// the image pixels section.
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Image {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) frames: u32,
    pub(crate) rate: f32,
    pub(crate) pixels: Range,
}

/// An array: its items in the values section, each a scalar's bits, or a
/// color's or a resource's index, read as the program types it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Array {
    pub(crate) items: Range,
}

pub(crate) const ENCODING_RGB: u32 = 0;
pub(crate) const ENCODING_RGBW: u32 = 1;
/// Each channel lights one color of the route's [`Palette`].
pub(crate) const ENCODING_CHANNELS: u32 = 2;

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
    /// The colors of an [`ENCODING_CHANNELS`] route's channels, or [`NONE`].
    pub(crate) palette: u32,
    /// [`ROUTE_DITHER`] and [`ROUTE_REVERSE`].
    pub(crate) flags: u32,
}

/// Its bytes dither over frames.
pub(crate) const ROUTE_DITHER: u32 = 1;
/// Its pixels fill its slots from the last to the first: pixel `end - 1`
/// takes the first slot.
pub(crate) const ROUTE_REVERSE: u32 = 2;

/// The color each channel of an [`ENCODING_CHANNELS`] route lights, packed
/// as `0x00rrggbb`: the first `count` are its channels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Palette {
    pub(crate) count: u32,
    pub(crate) colors: [u32; 4],
}

/// A route's gamma and brightness: the output at each input byte, in 1/256 of
/// a byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Lookup {
    pub(crate) table: [u16; 256],
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
    /// The Donder port it drives, or 0 for an E1.31 or Art-Net controller's
    /// channels across all its universes.
    pub port: u32,
    /// Where its bytes start in the frame's output bytes, after every
    /// output before it.
    pub(crate) offset: u32,
    /// Bytes.
    pub width: u32,
}

/// A live automation: the first of its three live store words (its value,
/// its integral's base, and the seconds that base holds from), and the value
/// it rests at until set. Its integral at `t` is `base + value * (t - since)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct LiveAutomation {
    pub(crate) word: u32,
    pub(crate) rest: f32,
}

/// A live store word holding live automation `automation`'s integral at
/// `clip`'s start, which the clip's integrals count from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct Snapshot {
    pub(crate) automation: u32,
    pub(crate) clip: u32,
    pub(crate) word: u32,
}

/// What a console's `On` answers.
pub(crate) const HAPPENING_PRESS: u32 = 0;
pub(crate) const HAPPENING_RELEASE: u32 = 1;
pub(crate) const HAPPENING_MOVE: u32 = 2;

/// The actions a happening of a control runs, in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct ConsoleOn {
    pub(crate) control: u32,
    pub(crate) happening: u32,
    pub(crate) actions: Range,
}

pub(crate) const ACTION_PLAY: u32 = 0;
pub(crate) const ACTION_START: u32 = 1;
pub(crate) const ACTION_STOP: u32 = 2;
pub(crate) const ACTION_TOGGLE: u32 = 3;
pub(crate) const ACTION_MARK: u32 = 4;
pub(crate) const ACTION_SET: u32 = 5;

/// How long a copy plays, or a held copy plays before it repeats:
/// `shortest`, or from `shortest` to `longest` by the velocity or an axis.
pub(crate) const LENGTH_FIXED: u32 = 0;
pub(crate) const LENGTH_VELOCITY: u32 = 1;
pub(crate) const LENGTH_AXIS: u32 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct ConsoleAction {
    pub(crate) kind: u32,
    /// The console clip a play, start, stop or toggle names, the live mark
    /// collection a mark adds to, or the live automation a set writes.
    pub(crate) target: u32,
    /// The most copies a play plays at once.
    pub(crate) copies: u32,
    pub(crate) length: u32,
    /// The control a [`LENGTH_AXIS`] length reads.
    pub(crate) axis: u32,
    /// Ticks.
    pub(crate) shortest: u32,
    pub(crate) longest: u32,
    /// The part of `0..1` a set's value maps through.
    pub(crate) low: f32,
    pub(crate) high: f32,
}

pub(crate) const PLAYED_VELOCITY: u32 = 0;
pub(crate) const PLAYED_PITCH_CLASS: u32 = 1;

/// A parameter a copy takes from the press that started it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct PlayedValue {
    pub(crate) source: u32,
    pub(crate) low: f32,
    pub(crate) high: f32,
    /// The live automation that sets it on each of the clip's voices, in
    /// console indices.
    pub(crate) automations: Range,
}

/// A console clip: its voices, in console indices, and its played values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub(crate) struct ConsoleClip {
    pub(crate) voices: Range,
    pub(crate) values: Range,
}

pub(crate) use donder_runtime_types::InstructionWord;
pub(crate) use donder_runtime_types::sampling::CrossingSegment;
pub(crate) use donder_runtime_types::{Color, CurvePoint};

macro_rules! sections {
    ($($name:ident: $ty:ty,)+) => {
        /// Every section of a show, borrowed from its payload.
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
            /// The show: its header, section table and payload, each section
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
    pixel_structure: PixelStructure,
    clips: Clip,
    clip_index: ClipIndex,
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
    resources: u32,
    curves: Curve,
    curve_points: CurvePoint,
    curve_areas: f32,
    curve_slopes: f32,
    crossings: CrossingSegment,
    gradients: Gradient,
    gradient_stops: GradientStop,
    marks: Marks,
    mark_ticks: u32,
    audio_tracks: AudioTrack,
    audio_samples: u8,
    fonts: Font,
    glyphs: donder_runtime_types::Glyph,
    glyph_coverage: u8,
    images: Image,
    image_pixels: u8,
    arrays: Array,
    values: u32,
    routes: Route,
    lookups: Lookup,
    palettes: Palette,
    outputs: Output,
    voices: u32,
    live_automations: LiveAutomation,
    snapshots: Snapshot,
    console_ons: ConsoleOn,
    console_actions: ConsoleAction,
    played_values: PlayedValue,
    console_clips: ConsoleClip,
    console_indices: u32,
    // The `Start` action the session runs for each `always` clip.
    console_always: ConsoleAction,
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
    };
    let mut bytes = alloc::vec::Vec::with_capacity(header_bytes() + payload.len());
    bytes.extend_from_slice(bytemuck::bytes_of(&header));
    bytes.extend_from_slice(table_bytes);
    bytes.extend_from_slice(payload);
    let mut words = alloc::vec![0u32; bytes.len().div_ceil(4)];
    bytemuck::cast_slice_mut::<u32, u8>(&mut words)[..bytes.len()].copy_from_slice(&bytes);
    words.into_boxed_slice()
}

/// The show's length from its header: the header, section table and
/// payload. Lets a receiver size the rest of a transfer.
pub(crate) fn show_len(header: &[u8]) -> usize {
    let header: Header = bytemuck::pod_read_unaligned(&header[..size_of::<Header>()]);
    header_bytes() + header.payload as usize
}

/// The show's identity from its header.
pub(crate) fn show_id(header: &[u8]) -> u32 {
    let header: Header = bytemuck::pod_read_unaligned(&header[..size_of::<Header>()]);
    header.id
}

/// The format version in the show's header, or `None` for bytes too short
/// for a header and section table.
pub(crate) fn version(bytes: &[u8]) -> Option<u32> {
    let header = bytes.get(..header_bytes())?;
    let header: Header = bytemuck::pod_read_unaligned(&header[..size_of::<Header>()]);
    Some(header.version)
}

/// The sections of a show whose header was checked when it was admitted.
/// The show is trusted: only its producer writes it. `bytes` must be 4-byte
/// aligned, as the writer's buffers and flash mappings are.
pub(crate) fn sections(bytes: &[u8]) -> Sections<'_> {
    let (header, payload) = bytes.split_at(header_bytes());
    let table = bytemuck::cast_slice(&header[size_of::<Header>()..]);
    Sections::borrow(payload, table)
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
