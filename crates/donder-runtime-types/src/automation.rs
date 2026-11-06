use alloc::vec::Vec;

use crate::sampling::sample_curve;
use crate::types::{Identifier, Type};
use crate::values::{Curve, CurvePoint};

/// How a binding sets its parameter: a normalized curve mapped onto the
/// parameter's range or options, or a console's live mark collection.
#[derive(Clone, Debug, PartialEq)]
pub enum AutomationMapping {
    Float {
        min: f32,
        max: f32,
    },
    Int {
        min: i32,
        max: i32,
    },
    Bool,
    Enum {
        values: Vec<Identifier>,
    },
    Curve {
        min: f32,
        max: f32,
    },
    /// A marks parameter that reads a live collection, whose marks playback
    /// adds as they arrive. Its binding's curve is unused.
    LiveMarks,
}

impl AutomationMapping {
    /// A hash consistent with `==`; floats hash by their bits.
    pub(crate) fn hash_same<H: core::hash::Hasher>(&self, state: &mut H) {
        use core::hash::Hash;
        core::mem::discriminant(self).hash(state);
        match self {
            Self::Float { min, max } | Self::Curve { min, max } => {
                min.to_bits().hash(state);
                max.to_bits().hash(state);
            }
            Self::Int { min, max } => {
                min.hash(state);
                max.hash(state);
            }
            Self::Bool | Self::LiveMarks => {}
            Self::Enum { values } => values.hash(state),
        }
    }

    pub(crate) fn is_well_formed(&self) -> bool {
        match self {
            Self::Float { min, max } | Self::Curve { min, max } => {
                min.is_finite() && max.is_finite()
            }
            Self::Int { .. } | Self::Bool | Self::LiveMarks => true,
            Self::Enum { values } => !values.is_empty(),
        }
    }

    pub(crate) fn accepts_type(&self, ty: &Type) -> bool {
        self.is_well_formed()
            && match (self, ty) {
                (Self::Float { .. }, Type::Float)
                | (Self::Int { .. }, Type::Int)
                | (Self::Bool, Type::Bool)
                | (Self::Curve { .. }, Type::Curve)
                | (Self::LiveMarks, Type::Marks) => true,
                (Self::Enum { values }, Type::Enum(options)) => {
                    values.iter().all(|value| options.contains(value))
                }
                _ => false,
            }
    }
}

pub enum AutomationValue<'a> {
    Int(i32),
    Float(f32),
    Bool(bool),
    Enum(&'a Identifier),
    Curve(Curve),
}

pub fn automation_value_at_position<'a>(
    curve: &Curve,
    mapping: &'a AutomationMapping,
    position: f32,
) -> Option<AutomationValue<'a>> {
    let amount = sample_curve(curve, position);
    Some(match mapping {
        AutomationMapping::Float { min, max } => {
            AutomationValue::Float(automated_float(*min, *max, amount))
        }
        AutomationMapping::Int { min, max } => {
            AutomationValue::Int(automated_int(*min, *max, amount))
        }
        AutomationMapping::Bool => AutomationValue::Bool(automated_bool(amount)),
        AutomationMapping::Enum { values } => {
            values.len().checked_sub(1)?;
            AutomationValue::Enum(&values[automated_index(values.len(), amount)])
        }
        AutomationMapping::Curve { min, max } => {
            let mut points =
                alloc::vec![CurvePoint { position: 0.0, value: 0.0 }; curve.points.len().max(1)];
            let len = curve_window_points(&mut points, &curve.points, *min, *max, position);
            points.truncate(len);
            AutomationValue::Curve(Curve { points })
        }
        AutomationMapping::LiveMarks => return None,
    })
}

/// The part of `curve` from `sample_position` on, mapped to `min..max` and
/// shifted to start at 0, into a buffer of at least `curve.len().max(1)`
/// points; returns how many it wrote.
pub fn curve_window_points(
    output: &mut [CurvePoint],
    curve: &[CurvePoint],
    min: f32,
    max: f32,
    sample_position: f32,
) -> usize {
    let mut len = 0;
    for point in curve {
        let position = point.position - sample_position;
        if (0.0..=1.0).contains(&position) {
            output[len] = CurvePoint {
                position,
                value: automated_float(min, max, point.value),
            };
            len += 1;
        }
    }
    if len == 0 {
        output[0] = CurvePoint {
            position: 0.0,
            value: automated_float(
                min,
                max,
                crate::sampling::sample_curve_points(curve, sample_position),
            ),
        };
        len = 1;
    }
    len
}

/// An automation amount in 0..1 as a value of `min..max`. Playback and the
/// editor both map automation through these.
#[inline]
pub fn automated_float(min: f32, max: f32, amount: f32) -> f32 {
    min + (max - min) * amount
}

#[inline]
pub fn automated_int(min: i32, max: i32, amount: f32) -> i32 {
    libm::roundf(automated_float(min as f32, max as f32, amount)) as i32
}

#[inline]
pub fn automated_bool(amount: f32) -> bool {
    amount >= 0.5
}

/// Which of `count` options, one or more, an amount picks.
#[inline]
pub fn automated_index(count: usize, amount: f32) -> usize {
    (libm::floorf(amount * count as f32) as usize).min(count - 1)
}
