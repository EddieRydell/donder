use crate::dsl::{Identifier, ParamDecl, Value};
use crate::effect::{CurveSource, EffectParamValue, GradientSource};
use crate::model::DonderProject;
use crate::sequence::{MarkCollection, MarkCollectionKey, Sequence};
use crate::values::{Marks, SampleDuration, SampleTime};
use indexmap::IndexMap;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) struct EffectParamTiming {
    pub(crate) start: SampleTime,
    pub(crate) duration: SampleDuration,
}

pub(crate) fn prepare_params(
    project: &DonderProject,
    sequence: &Sequence,
    declarations: &[ParamDecl],
    overrides: &IndexMap<Identifier, EffectParamValue>,
    timing: EffectParamTiming,
) -> IndexMap<Identifier, Value> {
    let collections = sequence
        .mark_collections
        .iter()
        .map(|collection| (&collection.key, collection))
        .collect();
    let mut params = declarations
        .iter()
        .filter_map(|param| {
            param
                .default
                .as_ref()
                .map(|value| (param.name.clone(), value.clone()))
        })
        .collect::<IndexMap<_, _>>();
    params.extend(overrides.iter().map(|(key, value)| {
        (
            key.clone(),
            prepare_param_value(project, &collections, value, timing),
        )
    }));
    params
}

fn prepare_param_value(
    project: &DonderProject,
    collections: &IndexMap<&MarkCollectionKey, &MarkCollection>,
    value: &EffectParamValue,
    timing: EffectParamTiming,
) -> Value {
    // Loading/edit acceptance establishes reference existence and timing ranges.
    // Here we only turn those authored values into their playback representation.
    match value {
        EffectParamValue::Int(value) => Value::Int(*value),
        EffectParamValue::Float(value) => Value::Float(*value),
        EffectParamValue::Bool(value) => Value::Bool(*value),
        EffectParamValue::Color(value) => Value::Color(*value),
        EffectParamValue::Enum(value) => Value::Enum(value.clone()),
        EffectParamValue::Marks(key) => {
            let collection = collections[key];
            let start = u64::from(timing.start.as_ticks());
            let end = start + u64::from(timing.duration.as_ticks());
            Value::Marks(Arc::new(Marks {
                marks: collection
                    .marks
                    .iter()
                    .filter_map(|mark| {
                        let mark = mark.as_micros_rounded();
                        (mark >= u128::from(start) && mark < u128::from(end))
                            .then(|| SampleDuration::from_ticks((mark - u128::from(start)) as u32))
                    })
                    .collect(),
            }))
        }
        EffectParamValue::Curve(source) => Value::Curve(Arc::new(match source {
            CurveSource::Inline(curve) => curve.clone(),
            CurveSource::Reference(id) => {
                project.definitions().curves.definitions[id].curve.clone()
            }
        })),
        EffectParamValue::Gradient(source) => Value::Gradient(Arc::new(match source {
            GradientSource::Inline(gradient) => gradient.clone(),
            GradientSource::Reference(id) => project.definitions().gradients.definitions[id]
                .gradient
                .clone(),
        })),
        EffectParamValue::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| prepare_param_value(project, collections, value, timing))
                .collect::<Vec<_>>()
                .into(),
        ),
    }
}
