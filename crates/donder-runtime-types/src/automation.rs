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
    let normalized = sample_curve(curve, position);
    Some(match mapping {
        AutomationMapping::Float { min, max } => {
            AutomationValue::Float(lerp(*min, *max, normalized))
        }
        AutomationMapping::Int { min, max } => {
            AutomationValue::Int(libm::roundf(lerp(*min as f32, *max as f32, normalized)) as i32)
        }
        AutomationMapping::Bool => AutomationValue::Bool(normalized >= 0.5),
        AutomationMapping::Enum { values } => {
            let index = (libm::floorf(normalized * values.len() as f32) as usize)
                .min(values.len().checked_sub(1)?);
            AutomationValue::Enum(&values[index])
        }
        AutomationMapping::Curve { min, max } => {
            let mut output = Curve { points: Vec::new() };
            curve_window_into(&mut output, curve, *min, *max, position);
            AutomationValue::Curve(output)
        }
        AutomationMapping::LiveMarks => return None,
    })
}

pub fn curve_window_into(
    output: &mut Curve,
    curve: &Curve,
    min: f32,
    max: f32,
    sample_position: f32,
) {
    output.points.clear();
    output
        .points
        .extend(curve.points.iter().filter_map(|point| {
            let position = point.position - sample_position;
            (0.0..=1.0).contains(&position).then(|| CurvePoint {
                position,
                value: lerp(min, max, point.value),
            })
        }));
    if output.points.is_empty() {
        output.points.push(CurvePoint {
            position: 0.0,
            value: lerp(min, max, sample_curve(curve, sample_position)),
        });
    }
}

/// [`curve_window_into`] into a buffer of at least `curve.len().max(1)`
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
                value: lerp(min, max, point.value),
            };
            len += 1;
        }
    }
    if len == 0 {
        output[0] = CurvePoint {
            position: 0.0,
            value: lerp(
                min,
                max,
                crate::sampling::sample_curve_points(curve, sample_position),
            ),
        };
        len = 1;
    }
    len
}

fn lerp(min: f32, max: f32, amount: f32) -> f32 {
    min + (max - min) * amount
}
