//! Portable construction descriptions shared by authoring admission and playback.
mod target;
use crate::Shared as Arc;
use crate::automation::AutomationMapping;
use crate::values::{Curve, SampleDuration, SampleTime};
use alloc::boxed::Box;
use core::num::NonZeroU32;
pub use target::{TargetGeometry, TargetPixel};

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
    positions: Box<[[f32; 2]]>,
    cells: Box<[u32]>,
    retain_empty: bool,
}

impl FixtureGeometry {
    pub fn positions(&self) -> &[[f32; 2]] {
        &self.positions
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
            retain_empty: false,
            cells: self
                .cells
                .iter()
                .copied()
                .filter(|&cell| keep(cell))
                .collect(),
        }
    }

    pub fn admit(positions: Box<[[f32; 2]]>) -> Option<Self> {
        u32::try_from(positions.len()).ok()?;
        if positions.iter().flatten().any(|value| !value.is_finite()) {
            return None;
        }
        let cells = (0..positions.len() as u32).collect();
        Some(Self {
            positions,
            cells,
            retain_empty: true,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelEncoding {
    Rgb { order: [u8; 3] },
    Rgbw { order: [u8; 4] },
}

impl PixelEncoding {
    pub fn channel_order(&self) -> &[u8] {
        match self {
            Self::Rgb { order } => order,
            Self::Rgbw { order } => order,
        }
    }
    pub fn is_valid(&self) -> bool {
        let order = self.channel_order();
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
        self.0.channel_order().len()
    }

    pub fn encoding(self) -> PixelEncoding {
        self.0
    }
}

/// Layout-space position and the bounds of this sampling scope, in meters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialContext {
    pub position: [f32; 2],
    pub min: [f32; 2],
    pub max: [f32; 2],
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
