use crate::effect::{OwnedCurve, OwnedGradient, ParamValue};
use crate::project::DonderProject;
use crate::sequence::{MarkCollection, MarkCollectionKey};
use donder_language::compiler::ParamDecl;
use donder_runtime_types::{Identifier, Value};
use donder_runtime_types::{Marks, SampleDuration, SampleTime};
use indexmap::IndexMap;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) struct ClipTiming {
    pub(crate) start: SampleTime,
    pub(crate) duration: SampleDuration,
}

/// Where a marks parameter's collection comes from.
#[derive(Clone, Copy)]
pub(crate) enum MarkOrigin<'a> {
    /// A sequence's authored collections.
    Authored(&'a [MarkCollection]),
    /// A rig's live collections, which start empty; playback rebinds them.
    Live,
}

pub(crate) fn prepare_params(
    project: &DonderProject,
    marks: MarkOrigin<'_>,
    declarations: &[ParamDecl],
    overrides: &IndexMap<Identifier, ParamValue>,
    timing: ClipTiming,
) -> IndexMap<Identifier, Value> {
    let collections = match marks {
        MarkOrigin::Authored(collections) => collections
            .iter()
            .map(|collection| (&collection.key, collection))
            .collect(),
        MarkOrigin::Live => IndexMap::new(),
    };
    let live = matches!(marks, MarkOrigin::Live);
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
            prepare_param_value(project, &collections, live, value, timing),
        )
    }));
    params
}

fn prepare_param_value(
    project: &DonderProject,
    collections: &IndexMap<&MarkCollectionKey, &MarkCollection>,
    live: bool,
    value: &ParamValue,
    timing: ClipTiming,
) -> Value {
    // Loading/edit acceptance establishes reference existence and timing ranges.
    // Here we only turn those authored values into their playback representation.
    match value {
        ParamValue::Int(value) => Value::Int(*value),
        ParamValue::Float(value) => Value::Float(*value),
        ParamValue::Bool(value) => Value::Bool(*value),
        ParamValue::Color(value) => Value::Color(*value),
        ParamValue::Enum(value) => Value::Enum(value.clone()),
        ParamValue::Marks(None) => Value::Marks(Arc::new(Marks::empty())),
        ParamValue::Marks(Some(_)) if live => Value::Marks(Arc::new(Marks::empty())),
        ParamValue::Marks(Some(key)) => {
            // The collection's whole track; preparation shares equal tracks.
            let mut track: Vec<u32> = collections[key]
                .marks
                .iter()
                .filter_map(|mark| u32::try_from(mark.time.as_micros_rounded()).ok())
                .collect();
            track.sort_unstable();
            let start = timing.start.as_ticks();
            let end = start.saturating_add(timing.duration.as_ticks());
            Value::Marks(Arc::new(Marks::window(track.into(), start, end)))
        }
        ParamValue::Curve(source) => Value::Curve(Arc::new(match source {
            OwnedCurve::Inline(curve) => curve.clone(),
            OwnedCurve::Reference(id) => project.reusable::<crate::CurveDefinition>()[id]
                .curve
                .clone(),
        })),
        ParamValue::Gradient(source) => Value::Gradient(Arc::new(match source {
            OwnedGradient::Inline(gradient) => gradient.clone(),
            OwnedGradient::Reference(id) => project.reusable::<crate::GradientDefinition>()[id]
                .gradient
                .clone(),
        })),
        ParamValue::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| prepare_param_value(project, collections, live, value, timing))
                .collect::<Vec<_>>()
                .into(),
        ),
    }
}
