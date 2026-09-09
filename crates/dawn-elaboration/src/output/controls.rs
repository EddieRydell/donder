use std::collections::HashMap;

use dawn_language::control::{
    ControlClip, ControlResolutionError, ControlTarget, ControlValue, resolve_controls,
};
use dawn_language::element::{ElementNodeKind, ElementTree};
use dawn_language::fixture_profile::{
    FixtureBehaviorRule, FixtureControlValue, FixtureFunctionKind, FixtureProfileId,
    FixtureProfileStore,
};
use dawn_language::values::{sample_duration_from_dawn_duration, sample_time_from_dawn_time};

use super::elements::OutputElements;
use super::errors::SequenceOutputPrepareError;
use dawn_runtime::control::{
    PreparedControl, PreparedControlAddress, PreparedControlKind, PreparedControlValue,
};
use dawn_runtime::fixture::{FixtureBehavior, FixtureBehaviors};

pub(crate) fn prepare_controls(
    tree: &ElementTree,
    profiles: &FixtureProfileStore,
    clips: &[ControlClip],
    elements: &OutputElements,
) -> Result<Vec<PreparedControl>, SequenceOutputPrepareError> {
    let mut prepared = Vec::with_capacity(clips.len());
    let resolved = resolve_controls(tree, profiles, clips).map_err(|error| match error {
        ControlResolutionError::Conflict {
            first,
            second,
            address,
        } => SequenceOutputPrepareError::ControlConflict {
            first: first.0,
            second: second.0,
            node: address.node,
            cell: address.cell,
        },
        ControlResolutionError::InvalidTarget { clip, .. } => {
            SequenceOutputPrepareError::InvalidControl {
                clip: clip.0,
                reason: error.to_string(),
            }
        }
        ControlResolutionError::InvalidClip(error) => {
            SequenceOutputPrepareError::ProjectValidation(format!(
                "Invalid control clip: {error:?}"
            ))
        }
    })?;
    for (clip, addresses) in clips.iter().zip(resolved) {
        let start = sample_time_from_dawn_time(&clip.start).map_err(|_| {
            SequenceOutputPrepareError::InvalidControl {
                clip: clip.id.0,
                reason: "control start exceeds the runtime clock range".to_string(),
            }
        })?;
        let duration = sample_duration_from_dawn_duration(&clip.duration).map_err(|_| {
            SequenceOutputPrepareError::InvalidControl {
                clip: clip.id.0,
                reason: "control duration exceeds the runtime clock range".to_string(),
            }
        })?;
        let kind = match clip.target {
            ControlTarget::Scalar(_) => PreparedControlKind::Scalar,
            ControlTarget::Indexed(_) => PreparedControlKind::Indexed,
            ControlTarget::FixtureFunction { function, .. } => {
                PreparedControlKind::Fixture(function)
            }
        };
        let addresses = addresses
            .into_iter()
            .map(|address| {
                Ok(PreparedControlAddress {
                    element: *elements.indexes.get(&address.node).ok_or_else(|| {
                        SequenceOutputPrepareError::InvalidControl {
                            clip: clip.id.0,
                            reason: "control target is not a renderable element".to_string(),
                        }
                    })?,
                    cell: address.cell,
                })
            })
            .collect::<Result<Box<[_]>, _>>()?;
        prepared.push(PreparedControl {
            id: clip.id.0,
            start,
            duration,
            kind,
            value: match &clip.value {
                ControlValue::ConstantNormalized(value) => {
                    PreparedControlValue::ConstantNormalized(*value)
                }
                ControlValue::NormalizedCurve(curve) => {
                    PreparedControlValue::NormalizedCurve(curve.points.clone().into_boxed_slice())
                }
                ControlValue::Indexed { option, .. } => {
                    PreparedControlValue::Indexed { option: option.0 }
                }
                ControlValue::FixtureIndexed { entry, range_curve } => {
                    PreparedControlValue::FixtureIndexed {
                        entry: *entry,
                        range_curve: range_curve
                            .as_ref()
                            .map(|curve| curve.points.clone().into_boxed_slice()),
                    }
                }
                ControlValue::ConstantColor(color) => PreparedControlValue::ConstantColor(*color),
                ControlValue::Gradient(gradient) => {
                    PreparedControlValue::Gradient(gradient.stops.clone().into_boxed_slice())
                }
            },
            addresses,
        });
    }
    Ok(prepared)
}

pub(crate) fn prepare_fixture_behaviors(
    tree: &ElementTree,
    profiles: &FixtureProfileStore,
    elements: &OutputElements,
) -> Result<FixtureBehaviors, SequenceOutputPrepareError> {
    let mut rules = Vec::new();
    let mut bindings = Vec::new();
    let mut ranges = HashMap::<FixtureProfileId, core::ops::Range<u32>>::new();
    for (element, (id, _)) in elements.layouts.iter().enumerate() {
        let node = &tree.nodes[id];
        let ElementNodeKind::Fixture { profile } = &node.kind else {
            continue;
        };
        let element = u32::try_from(element).map_err(|_| {
            SequenceOutputPrepareError::InvalidPatch(
                "fixture element index exceeds u32".to_string(),
            )
        })?;
        if let Some(range) = ranges.get(profile) {
            if !range.is_empty() {
                bindings.push((element, range.clone()));
            }
            continue;
        }
        let profile_id = profile.clone();
        // Every previously appended block has already passed the checked end conversion.
        let start = rules.len() as u32;
        let profile = profiles.definitions.get(profile).ok_or_else(|| {
            SequenceOutputPrepareError::InvalidPatch("fixture profile is missing".to_string())
        })?;
        for (function, definition) in &profile.functions {
            if matches!(definition.kind, FixtureFunctionKind::ColorMixing { .. }) {
                rules.push((*function, FixtureBehavior::Color));
            }
        }
        for rule in &profile.behavior_rules {
            let (function, behavior) = match rule {
                FixtureBehaviorRule::Shutter {
                    function,
                    closed: off,
                    open: on,
                }
                | FixtureBehaviorRule::PrismGate {
                    function,
                    disabled: off,
                    enabled: on,
                } => (
                    *function,
                    FixtureBehavior::Switch {
                        off: FixtureControlValue::Indexed {
                            entry: *off,
                            range: 0.0,
                        },
                        on: FixtureControlValue::Indexed {
                            entry: *on,
                            range: 0.0,
                        },
                    },
                ),
                FixtureBehaviorRule::Dimmer { function, off, on } => (
                    *function,
                    FixtureBehavior::Switch {
                        off: FixtureControlValue::Normalized(*off),
                        on: FixtureControlValue::Normalized(*on),
                    },
                ),
                FixtureBehaviorRule::ColorWheel { function, entries } => (
                    *function,
                    FixtureBehavior::ColorWheel(
                        entries
                            .iter()
                            .map(|entry| (entry.color, entry.entry))
                            .collect(),
                    ),
                ),
            };
            rules.push((function, behavior));
        }
        let end = u32::try_from(rules.len()).map_err(|_| {
            SequenceOutputPrepareError::InvalidPatch(
                "fixture behavior table exceeds u32".to_string(),
            )
        })?;
        let range = start..end;
        if !range.is_empty() {
            bindings.push((element, range.clone()));
        }
        ranges.insert(profile_id, range);
    }
    Ok(FixtureBehaviors {
        bindings: bindings.into_boxed_slice(),
        rules: rules.into_boxed_slice(),
    })
}
