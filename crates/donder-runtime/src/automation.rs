use alloc::vec::Vec;

use crate::dsl::Identifier;
use crate::sampling::sample_curve;
use crate::values::{Curve, CurvePoint};

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum AutomationMapping {
    Float { min: f32, max: f32 },
    Int { min: i32, max: i32 },
    Bool,
    Enum { values: Vec<Identifier> },
    Curve { min: f32, max: f32 },
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
    let normalized = sample_curve(curve, position).clamp(0.0, 1.0);
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

fn lerp(min: f32, max: f32, amount: f32) -> f32 {
    min + (max - min) * amount.clamp(0.0, 1.0)
}
