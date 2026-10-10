use crate::effect::{CurveSource, EffectParamValue, GradientSource};
use crate::project::DonderProject;
use crate::sequence::{MarkCollection, MarkCollectionKey, Sequence};
use donder_language::compiler::ParamDecl;
use donder_runtime_types::{Identifier, Value};
use donder_runtime_types::{Marks, SampleDuration, SampleTime};
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
        EffectParamValue::Marks(None) => Value::Marks(Arc::new(Marks::empty())),
        EffectParamValue::Marks(Some(key)) => {
            // The collection's whole track; preparation shares equal tracks.
            let mut track: Vec<u32> = collections[key]
                .marks
                .iter()
                .filter_map(|mark| u32::try_from(mark.as_micros_rounded()).ok())
                .collect();
            track.sort_unstable();
            let start = timing.start.as_ticks();
            let end = start.saturating_add(timing.duration.as_ticks());
            Value::Marks(Arc::new(Marks::window(track.into(), start, end)))
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
