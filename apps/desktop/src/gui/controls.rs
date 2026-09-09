use super::{
    GuiMutationError,
    model::{
        curve_from_points, curve_points, gradient_from_stops, gradient_stops, parse_color,
        sequence_mut,
    },
};
use crate::dto::*;
use dawn_language::control::authoring::{ControlChannelOptions, control_channels};
use dawn_language::control::{ControlClip, ControlClipId, ControlTarget, ControlValue};
use dawn_language::element::{ElementCellRange, ElementNodeId, ElementSelection, IndexedOptionId};
use dawn_language::fixture_profile::{FixtureEntryId, FixtureFunctionId};
use dawn_language::sequence::SequenceId;
use dawn_language::values::{DawnDuration, DawnTime};
use dawn_project_io::{ProjectSession, SourceObjectKind};

pub(super) fn channels(session: &ProjectSession) -> Result<Vec<SequenceControlChannel>, String> {
    let tree = super::projection::active_element_tree(session).ok_or("Element tree is missing.")?;
    let channels = control_channels(tree, &session.project.definitions.fixture_profiles)
        .map_err(|error| format!("Invalid control targets: {error:?}"))?;
    Ok(channels
        .into_iter()
        .map(|channel| SequenceControlChannel {
            target: project_target(&channel.target),
            label: channel.label,
            cell_count: channel.cell_count,
            options: match channel.options {
                ControlChannelOptions::Normalized => SequenceControlOptions::Normalized,
                ControlChannelOptions::Color => SequenceControlOptions::Color,
                ControlChannelOptions::Indexed(options) => SequenceControlOptions::Indexed {
                    options: options
                        .into_iter()
                        .map(|option| SetupIndexedOption {
                            id: option.id.0,
                            name: option.name,
                        })
                        .collect(),
                },
                ControlChannelOptions::FixtureIndexed(entries) => {
                    SequenceControlOptions::FixtureIndexed {
                        entries: entries
                            .into_iter()
                            .map(|entry| SequenceControlEntry {
                                id: entry.id.0,
                                name: entry.name,
                                range_control: entry.curve_control,
                            })
                            .collect(),
                    }
                }
            },
        })
        .collect())
}

pub(super) fn project_target(target: &ControlTarget) -> SequenceControlTarget {
    let selection = target.selection();
    let node = selection.node.0;
    let cells = selection.cells.map(|range| PatchGuiCellRange {
        start: range.start,
        count: range.count,
    });
    match target {
        ControlTarget::Scalar(_) => SequenceControlTarget::Scalar { node, cells },
        ControlTarget::Indexed(_) => SequenceControlTarget::Indexed { node, cells },
        ControlTarget::FixtureFunction { function, .. } => SequenceControlTarget::FixtureFunction {
            node,
            cells,
            function: function.0,
        },
    }
}

pub(super) fn project_value(value: &ControlValue) -> SequenceControlValue {
    match value {
        ControlValue::ConstantNormalized(value) => {
            SequenceControlValue::ConstantNormalized { value: *value }
        }
        ControlValue::NormalizedCurve(curve) => SequenceControlValue::NormalizedCurve {
            points: curve_points(curve),
        },
        ControlValue::Indexed {
            option,
            range_curve,
        } => SequenceControlValue::Indexed {
            option: option.0,
            range_curve: range_curve.as_ref().map(curve_points),
        },
        ControlValue::FixtureIndexed { entry, range_curve } => {
            SequenceControlValue::FixtureIndexed {
                entry: entry.0,
                range_curve: range_curve.as_ref().map(curve_points),
            }
        }
        ControlValue::ConstantColor(color) => SequenceControlValue::ConstantColor {
            value: color.to_hex(),
        },
        ControlValue::Gradient(gradient) => SequenceControlValue::Gradient {
            stops: gradient_stops(gradient),
        },
    }
}

pub(super) fn upsert(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    id: Option<u32>,
    start_seconds: f32,
    duration_seconds: f32,
    target: SequenceControlTarget,
    value: SequenceControlValue,
) -> Result<(), GuiMutationError> {
    let tree = super::projection::active_element_tree(session)
        .ok_or_else(|| GuiMutationError::Invalid("Element tree is missing.".into()))?
        .id
        .clone();
    let selection = |node, cells: Option<PatchGuiCellRange>| ElementSelection {
        tree: tree.clone(),
        node: ElementNodeId(node),
        cells: cells.map(|range| ElementCellRange {
            start: range.start,
            count: range.count,
        }),
    };
    let target = match target {
        SequenceControlTarget::Scalar { node, cells } => {
            ControlTarget::Scalar(selection(node, cells))
        }
        SequenceControlTarget::Indexed { node, cells } => {
            ControlTarget::Indexed(selection(node, cells))
        }
        SequenceControlTarget::FixtureFunction {
            node,
            cells,
            function,
        } => ControlTarget::FixtureFunction {
            selection: selection(node, cells),
            function: FixtureFunctionId(function),
        },
    };
    let value = match value {
        SequenceControlValue::ConstantNormalized { value } => {
            ControlValue::ConstantNormalized(value)
        }
        SequenceControlValue::NormalizedCurve { points } => {
            ControlValue::NormalizedCurve(curve_from_points(points))
        }
        SequenceControlValue::Indexed {
            option,
            range_curve,
        } => ControlValue::Indexed {
            option: IndexedOptionId(option),
            range_curve: range_curve.map(curve_from_points),
        },
        SequenceControlValue::FixtureIndexed { entry, range_curve } => {
            ControlValue::FixtureIndexed {
                entry: FixtureEntryId(entry),
                range_curve: range_curve.map(curve_from_points),
            }
        }
        SequenceControlValue::ConstantColor { value } => {
            ControlValue::ConstantColor(parse_color(&value)?)
        }
        SequenceControlValue::Gradient { stops } => {
            ControlValue::Gradient(gradient_from_stops(stops)?)
        }
    };
    let start = std::time::Duration::try_from_secs_f32(start_seconds).map_err(|_| {
        GuiMutationError::Invalid("Control start must be a finite, nonnegative time.".into())
    })?;
    let duration = std::time::Duration::try_from_secs_f32(duration_seconds).map_err(|_| {
        GuiMutationError::Invalid("Control duration must be a finite, positive time.".into())
    })?;
    dawn_project_io::ensure_document_can_reference_source(
        session,
        sequence_id.0.document_id(),
        SourceObjectKind::ElementTree,
        &tree.0,
    )
    .map_err(|error| GuiMutationError::Invalid(format!("{error:?}")))?;
    let sequence = sequence_mut(session, sequence_id)?;
    let next_id = match id {
        Some(id) => id,
        None => sequence
            .control_clips
            .iter()
            .map(|clip| clip.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| {
                GuiMutationError::Invalid("No control clip identifiers remain.".into())
            })?,
    };
    let clip = ControlClip {
        id: ControlClipId(next_id),
        start: DawnTime(start),
        duration: DawnDuration(duration),
        target,
        value,
    };
    if id.is_some() {
        let current = sequence
            .control_clips
            .iter_mut()
            .find(|clip| clip.id.0 == next_id)
            .ok_or_else(|| GuiMutationError::Invalid("Control clip was not found.".into()))?;
        *current = clip;
    } else {
        sequence.control_clips.push(clip);
    }
    Ok(())
}
