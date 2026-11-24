//! Portable construction descriptions shared by authoring admission and playback.
mod target;
use crate::Shared as Arc;
use crate::automation::AutomationMapping;
use crate::values::{Curve, SampleDuration, SampleTime};
use alloc::boxed::Box;
use core::num::NonZeroU32;
pub use target::{PixelStructure, TargetGeometry, TargetPixel};

/// An accepted show clock.
/// Cache this at project admission so host construction cannot fail on timing.
#[derive(Clone, Debug)]
pub struct SequenceTiming {
    frame_rate: NonZeroU32,
    frame_count: u32,
    duration: NonZeroU32,
}

impl SequenceTiming {
    pub fn frame_rate(&self) -> u32 {
        self.frame_rate.get()
    }
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }
    pub fn duration(&self) -> SampleDuration {
        SampleDuration::from_ticks(self.duration.get())
    }

    pub fn new(frame_rate: NonZeroU32, frame_count: NonZeroU32, duration: NonZeroU32) -> Self {
        Self {
            frame_rate,
            frame_count: frame_count.get(),
            duration,
        }
    }
}

/// Finite geometry with representable fixture-local storage addresses.
#[derive(Clone, Debug)]
pub struct FixtureGeometry {
    positions: Box<[[f32; 3]]>,
    /// Each pixel's strand and its place along it.
    strands: Box<[(u32, u32)]>,
    /// Each strand's pixels.
    strand_lengths: Box<[u32]>,
    cells: Box<[u32]>,
    retain_empty: bool,
}

impl FixtureGeometry {
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// Each pixel's strand and its place along it, by cell.
    pub fn strands(&self) -> &[(u32, u32)] {
        &self.strands
    }

    /// Each strand's pixels.
    pub fn strand_lengths(&self) -> &[u32] {
        &self.strand_lengths
    }

    /// Original fixture-local cells retained in physical storage, in order.
    pub fn cells(&self) -> &[u32] {
        &self.cells
    }

    /// Full inventories retain even empty fixture records; selected inventories
    /// omit fixtures that contribute no physical cells.
    pub fn retains_fixture(&self) -> bool {
        self.retain_empty || !self.cells.is_empty()
    }

    /// Select storage without changing the original sampling domain.
    pub fn select(&self, mut keep: impl FnMut(u32) -> bool) -> Self {
        Self {
            positions: self.positions.clone(),
            strands: self.strands.clone(),
            strand_lengths: self.strand_lengths.clone(),
            retain_empty: false,
            cells: self
                .cells
                .iter()
                .copied()
                .filter(|&cell| keep(cell))
                .collect(),
        }
    }

    pub fn admit(positions: Box<[[f32; 3]]>, strands: Box<[(u32, u32)]>) -> Option<Self> {
        u32::try_from(positions.len()).ok()?;
        if positions.iter().flatten().any(|value| !value.is_finite())
            || strands.len() != positions.len()
        {
            return None;
        }
        let count = strands
            .iter()
            .map(|(strand, _)| strand + 1)
            .max()
            .unwrap_or(0);
        let mut strand_lengths = alloc::vec![0; count as usize];
        for &(strand, _) in strands.iter() {
            strand_lengths[strand as usize] += 1;
        }
        let cells = (0..positions.len() as u32).collect();
        Some(Self {
            positions,
            strands,
            strand_lengths: strand_lengths.into(),
            cells,
            retain_empty: true,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelEncoding {
    Rgb {
        order: [u8; 3],
    },
    Rgbw {
        order: [u8; 4],
    },
    /// Lights that each show one color, one channel each: a dimmer, a
    /// single-color string, or one color of a set of colored strings.
    Channels {
        colors: ChannelColors,
    },
}

/// The most channels a [`PixelEncoding::Channels`] route gives a pixel.
pub const MAX_CHANNEL_COLORS: usize = 4;

/// The color each channel of a pixel lights, in channel order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelColors {
    count: u8,
    colors: [[u8; 3]; MAX_CHANNEL_COLORS],
}

impl ChannelColors {
    /// The colors, or none for no colors, more than [`MAX_CHANNEL_COLORS`],
    /// or black, which no channel can light.
    pub fn new(colors: &[[u8; 3]]) -> Option<Self> {
        if colors.is_empty() || colors.len() > MAX_CHANNEL_COLORS || colors.contains(&[0, 0, 0]) {
            return None;
        }
        let mut stored = [[0; 3]; MAX_CHANNEL_COLORS];
        stored[..colors.len()].copy_from_slice(colors);
        Some(Self {
            count: colors.len() as u8,
            colors: stored,
        })
    }

    pub fn as_slice(&self) -> &[[u8; 3]] {
        &self.colors[..usize::from(self.count)]
    }
}

/// A pixel's level, 0 to 1 before clamping, on a channel that lights
/// `channel` (each part 0..1): a gray channel lights at the pixel's
/// brightness, the most of its red, green and blue, scaled by the gray; a
/// colored channel at as much of its color as the pixel holds, the least of
/// the pixel's parts over the color's nonzero ones.
#[inline(always)]
pub fn channel_level(pixel: [f32; 3], channel: [f32; 3]) -> f32 {
    use crate::sampling::{max_float, min_float};
    if channel[0] == channel[1] && channel[1] == channel[2] {
        return max_float(max_float(pixel[0], pixel[1]), pixel[2]) * channel[0];
    }
    let mut level = 1.0;
    for (value, part) in pixel.iter().zip(&channel) {
        if *part > 0.0 {
            level = min_float(level, value / part);
        }
    }
    level
}

impl PixelEncoding {
    /// The channels each pixel takes.
    pub fn channel_count(&self) -> usize {
        match self {
            Self::Rgb { .. } => 3,
            Self::Rgbw { .. } => 4,
            Self::Channels { colors } => colors.as_slice().len(),
        }
    }

    pub fn is_valid(&self) -> bool {
        let order: &[u8] = match self {
            Self::Rgb { order } => order,
            Self::Rgbw { order } => order,
            Self::Channels { .. } => return true,
        };
        (0..order.len()).all(|channel| {
            order
                .iter()
                .filter(|&&value| usize::from(value) == channel)
                .count()
                == 1
        })
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TargetScope {
    PerFixture,
    WholeTarget,
}

/// A [`PixelEncoding`] whose channel order admission checked is a permutation.
#[derive(Clone, Copy)]
pub struct OutputEncoding(PixelEncoding);

impl OutputEncoding {
    pub fn admit(encoding: PixelEncoding) -> Option<Self> {
        encoding.is_valid().then_some(Self(encoding))
    }

    pub fn channel_count(self) -> usize {
        self.0.channel_count()
    }

    pub fn encoding(self) -> PixelEncoding {
        self.0
    }
}

/// Layout-space position and the bounds of this sampling scope, in meters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialContext {
    pub position: [f32; 3],
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct PreparedAutomation {
    pub start: SampleTime,
    pub duration: SampleDuration,
    pub curve: Arc<Curve>,
    pub mapping: AutomationMapping,
    pub quantity: AutomatedQuantity,
    pub param_index: u16,
}

/// What an automated slot holds: the parameter's value, or that float value
/// integrated over the definition's time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutomatedQuantity {
    Value,
    Integral,
}

impl PreparedAutomation {
    /// Bitwise equality: automation that drives its slot identically.
    pub fn same(&self, other: &Self) -> bool {
        self.start == other.start
            && self.duration == other.duration
            && self.curve.same(&other.curve)
            && self.mapping == other.mapping
            && self.quantity == other.quantity
            && self.param_index == other.param_index
    }

    /// A hash consistent with [`Self::same`].
    pub fn hash_same<H: core::hash::Hasher>(&self, state: &mut H) {
        use core::hash::Hash;
        self.start.as_ticks().hash(state);
        self.duration.as_ticks().hash(state);
        self.curve.hash_same(state);
        self.mapping.hash_same(state);
        (self.quantity == AutomatedQuantity::Integral).hash(state);
        self.param_index.hash(state);
    }
}
