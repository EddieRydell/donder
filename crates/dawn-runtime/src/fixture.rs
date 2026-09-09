use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::values::{Color, Curve};

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
    Hash,
    Ord,
    PartialOrd,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct FixtureFunctionId(pub u32);

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
    Hash,
    Ord,
    PartialOrd,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct FixtureEntryId(pub u32);

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum DimmingCurve {
    Linear,
    Gamma(f32),
    Custom(Curve),
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Hash, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum ColorComponent {
    Red,
    Green,
    Blue,
    White,
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum FixtureControlValue {
    Normalized(f32),
    Indexed { entry: FixtureEntryId, range: f32 },
    Color(Color),
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum FixtureBehavior {
    Color,
    Switch {
        off: FixtureControlValue,
        on: FixtureControlValue,
    },
    ColorWheel(Box<[(Color, FixtureEntryId)]>),
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct FixtureBehaviors {
    pub bindings: Box<[(u32, core::ops::Range<u32>)]>,
    pub rules: Box<[(FixtureFunctionId, FixtureBehavior)]>,
}

impl FixtureBehavior {
    pub fn sample(&self, color: Color) -> Option<FixtureControlValue> {
        match self {
            Self::Color => Some(FixtureControlValue::Color(color)),
            Self::Switch { off, on } => Some(
                if color.red | color.green | color.blue == 0 {
                    off
                } else {
                    on
                }
                .clone(),
            ),
            Self::ColorWheel(entries) => entries
                .iter()
                .find(|(candidate, _)| *candidate == color)
                .map(|(_, entry)| FixtureControlValue::Indexed {
                    entry: *entry,
                    range: 0.0,
                }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureState {
    pub functions: Vec<(FixtureFunctionId, FixtureControlValue)>,
}

impl FixtureState {
    pub fn get(&self, function: FixtureFunctionId) -> Option<&FixtureControlValue> {
        self.functions
            .iter()
            .find_map(|(id, value)| (*id == function).then_some(value))
    }

    pub fn insert(&mut self, function: FixtureFunctionId, value: FixtureControlValue) {
        if let Some((_, current)) = self.functions.iter_mut().find(|(id, _)| *id == function) {
            *current = value;
        } else {
            self.functions.push((function, value));
        }
    }
}

pub fn apply_dimming_curve(curve: &DimmingCurve, value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    match curve {
        DimmingCurve::Linear => value,
        DimmingCurve::Gamma(gamma) => libm::powf(value, *gamma),
        DimmingCurve::Custom(curve) => {
            let Some(first) = curve.points.first() else {
                return value;
            };
            if value <= first.position {
                return first.value;
            }
            for pair in curve.points.windows(2) {
                if value <= pair[1].position {
                    let span = pair[1].position - pair[0].position;
                    let amount = if span <= 0.0 {
                        0.0
                    } else {
                        (value - pair[0].position) / span
                    };
                    return pair[0].value + (pair[1].value - pair[0].value) * amount;
                }
            }
            curve.points.last().map_or(value, |point| point.value)
        }
    }
    .clamp(0.0, 1.0)
}

pub fn quantize8(value: f32) -> u8 {
    libm::roundf(value.clamp(0.0, 1.0) * 255.0) as u8
}
pub fn quantize16(value: f32) -> u16 {
    libm::roundf(value.clamp(0.0, 1.0) * 65_535.0) as u16
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct FixtureProgram {
    pub functions: Box<[FixtureFunction]>,
    pub channels: Box<[FixtureChannel]>,
    pub slot_count: u32,
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct FixtureFunction {
    pub id: FixtureFunctionId,
    pub curve: DimmingCurve,
    pub entries: Option<Box<[FixtureEntry]>>,
    pub has_fine: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct FixtureEntry {
    pub id: FixtureEntryId,
    pub min: u16,
    pub max: u16,
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct FixtureChannel {
    pub slot: u16,
    pub encoding: FixtureEncoding,
    pub curve: DimmingCurve,
}

#[derive(Clone, Copy, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum FixtureEncoding {
    Ignored,
    Color {
        function: FixtureFunctionId,
        component: ColorComponent,
        subtract_white: bool,
    },
    Value {
        function: u32,
        fine: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureEncodingError {
    TypeMismatch,
    MissingFunction,
    MissingEntry,
    InvalidSlot,
    WidthMismatch,
}

impl FixtureProgram {
    /// Writes into caller-owned storage; encoding never allocates or reads authoring metadata.
    /// State contains only functions driven in this frame. Undriven channels
    /// remain zero, including before and after a control clip's active interval.
    pub fn encode(
        &self,
        states: &[FixtureState],
        output: &mut [u8],
    ) -> Result<(), FixtureEncodingError> {
        let slot_count = self.slot_count as usize;
        if states.len().checked_mul(slot_count) != Some(output.len()) {
            return Err(FixtureEncodingError::WidthMismatch);
        }
        output.fill(0);
        for (index, state) in states.iter().enumerate() {
            let slots = &mut output[index * slot_count..(index + 1) * slot_count];
            for channel in &self.channels {
                let slot = slots
                    .get_mut(usize::from(channel.slot))
                    .ok_or(FixtureEncodingError::InvalidSlot)?;
                *slot = match channel.encoding {
                    FixtureEncoding::Ignored => 0,
                    FixtureEncoding::Color {
                        function,
                        component,
                        subtract_white,
                    } => {
                        let Some(value) = state.get(function) else {
                            continue;
                        };
                        let FixtureControlValue::Color(color) = value else {
                            return Err(FixtureEncodingError::TypeMismatch);
                        };
                        let rgb = [
                            f32::from(color.red) / 255.0,
                            f32::from(color.green) / 255.0,
                            f32::from(color.blue) / 255.0,
                        ];
                        let white = if subtract_white {
                            rgb[0].min(rgb[1]).min(rgb[2])
                        } else {
                            0.0
                        };
                        let value = match component {
                            ColorComponent::Red => rgb[0] - white,
                            ColorComponent::Green => rgb[1] - white,
                            ColorComponent::Blue => rgb[2] - white,
                            ColorComponent::White => white,
                        };
                        quantize8(apply_dimming_curve(&channel.curve, value))
                    }
                    FixtureEncoding::Value { function, fine } => {
                        let function = self
                            .functions
                            .get(function as usize)
                            .ok_or(FixtureEncodingError::MissingFunction)?;
                        let Some(value) = state.get(function.id) else {
                            continue;
                        };
                        let normalized = match value {
                            FixtureControlValue::Normalized(value) => {
                                apply_dimming_curve(&function.curve, *value)
                            }
                            FixtureControlValue::Indexed { entry, range } => {
                                let entries = function
                                    .entries
                                    .as_ref()
                                    .ok_or(FixtureEncodingError::TypeMismatch)?;
                                let index = entries
                                    .binary_search_by_key(entry, |entry| entry.id)
                                    .map_err(|_| FixtureEncodingError::MissingEntry)?;
                                let entry = &entries[index];
                                let dmx = f32::from(entry.min)
                                    + f32::from(entry.max - entry.min) * range.clamp(0.0, 1.0);
                                dmx / if function.has_fine { 65_535.0 } else { 255.0 }
                            }
                            FixtureControlValue::Color(_) => {
                                return Err(FixtureEncodingError::TypeMismatch);
                            }
                        };
                        // Preserve coarse-only encoding: its normalized value bypasses the channel curve.
                        if !fine && !function.has_fine {
                            quantize8(normalized)
                        } else {
                            let encoded =
                                quantize16(apply_dimming_curve(&channel.curve, normalized))
                                    .to_be_bytes();
                            encoded[usize::from(fine)]
                        }
                    }
                };
            }
        }
        Ok(())
    }
}
