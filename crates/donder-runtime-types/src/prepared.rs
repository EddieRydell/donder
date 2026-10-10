//! Portable construction descriptions shared by authoring admission and playback.
mod target;
use crate::Shared as Arc;
use crate::automation::AutomationMapping;
use crate::values::{Curve, SampleDuration, SampleTime};
use alloc::boxed::Box;
use core::num::NonZeroU32;
pub use target::{TargetGeometry, TargetPixel};

#[derive(Clone, Copy, Debug)]
pub struct SequenceWindow {
    pub start: SampleTime,
    pub duration: NonZeroU32,
}

/// An accepted sequence clock and its authored effect intervals.
/// Cache this at project admission so host construction cannot fail on timing.
#[derive(Clone, Debug)]
pub struct SequenceTiming {
    frame_rate: NonZeroU32,
    frame_count: u32,
    duration: NonZeroU32,
    windows: Box<[SequenceWindow]>,
}

impl SequenceTiming {
    pub fn windows(&self) -> &[SequenceWindow] {
        &self.windows
    }

    pub fn frame_rate(&self) -> u32 {
        self.frame_rate.get()
    }
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }
    pub fn duration(&self) -> SampleDuration {
        SampleDuration::from_ticks(self.duration.get())
    }

    pub fn admit(
        frame_rate: NonZeroU32,
        frame_count: NonZeroU32,
        duration: NonZeroU32,
        windows: Box<[SequenceWindow]>,
    ) -> Option<Self> {
        for window in &windows {
            let end = window.start.as_ticks().checked_add(window.duration.get())?;
            if end > duration.get() {
                return None;
            }
        }
        Some(Self {
            frame_rate,
            frame_count: frame_count.get(),
            duration,
            windows,
        })
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
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

#[derive(Clone, Copy)]
pub enum RgbOrder {
    Rgb,
    Rbg,
    Grb,
    Gbr,
    Brg,
    Bgr,
}

impl RgbOrder {
    fn admit(channels: [u8; 3]) -> Option<Self> {
        Some(match channels {
            [0, 1, 2] => Self::Rgb,
            [0, 2, 1] => Self::Rbg,
            [1, 0, 2] => Self::Grb,
            [1, 2, 0] => Self::Gbr,
            [2, 0, 1] => Self::Brg,
            [2, 1, 0] => Self::Bgr,
            _ => return None,
        })
    }

    fn channels(self) -> [u8; 3] {
        match self {
            Self::Rgb => [0, 1, 2],
            Self::Rbg => [0, 2, 1],
            Self::Grb => [1, 0, 2],
            Self::Gbr => [1, 2, 0],
            Self::Brg => [2, 0, 1],
            Self::Bgr => [2, 1, 0],
        }
    }
}

#[derive(Clone, Copy)]
pub enum WhitePosition {
    First,
    Second,
    Third,
    Fourth,
}

#[derive(Clone, Copy)]
pub enum OutputEncoding {
    Rgb(RgbOrder),
    Rgbw { rgb: RgbOrder, white: WhitePosition },
}

impl OutputEncoding {
    /// Admit the exact RGB/RGBW channel permutation from an authored route.
    pub fn admit(encoding: PixelEncoding) -> Option<Self> {
        Some(match encoding {
            PixelEncoding::Rgb { order } => Self::Rgb(RgbOrder::admit(order)?),
            PixelEncoding::Rgbw { order } => {
                let (channels, white) = match order {
                    [3, a, b, c] => ([a, b, c], WhitePosition::First),
                    [a, 3, b, c] => ([a, b, c], WhitePosition::Second),
                    [a, b, 3, c] => ([a, b, c], WhitePosition::Third),
                    [a, b, c, 3] => ([a, b, c], WhitePosition::Fourth),
                    _ => return None,
                };
                Self::Rgbw {
                    rgb: RgbOrder::admit(channels)?,
                    white,
                }
            }
        })
    }

    pub fn channel_count(self) -> usize {
        match self {
            Self::Rgb(_) => 3,
            Self::Rgbw { .. } => 4,
        }
    }

    pub fn encoding(self) -> PixelEncoding {
        match self {
            Self::Rgb(order) => PixelEncoding::Rgb {
                order: order.channels(),
            },
            Self::Rgbw { rgb, white } => {
                let [r, g, b] = rgb.channels();
                PixelEncoding::Rgbw {
                    order: match white {
                        WhitePosition::First => [3, r, g, b],
                        WhitePosition::Second => [r, 3, g, b],
                        WhitePosition::Third => [r, g, 3, b],
                        WhitePosition::Fourth => [r, g, b, 3],
                    },
                }
            }
        }
    }
}

/// Layout-space position and the bounds of this sampling scope, in meters.
#[derive(Clone, Copy, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct SpatialContext {
    pub position: [f32; 2],
    pub min: [f32; 2],
    pub max: [f32; 2],
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedAutomation {
    #[rkyv(with = crate::Microseconds)]
    pub start: SampleTime,
    #[rkyv(with = crate::Microseconds)]
    pub duration: SampleDuration,
    pub curve: Arc<Curve>,
    pub mapping: AutomationMapping,
    pub quantity: AutomatedQuantity,
    pub param_index: u16,
}

/// What an automated slot holds: the parameter's value, or that float value
/// integrated over the definition's time.
#[derive(Clone, Copy, Debug, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
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

    pub fn position(&self, sample_time: SampleTime) -> f32 {
        let elapsed = sample_time
            .checked_duration_since(self.start)
            .map_or(0, |duration| duration.as_ticks());
        if self.duration.as_ticks() == 0 {
            0.0
        } else {
            (elapsed as f32 / self.duration.as_ticks() as f32).clamp(0.0, 1.0)
        }
    }
}
