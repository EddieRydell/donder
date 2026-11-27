//! The translated clips, marks and song as a new sequence of the project.

use super::mapping::Mapping;
use super::translate::{self, Value};
use super::xsq::XSequence;
use super::{SequenceImportOptions, SequenceImportReport, XLIGHTS_EFFECTS};
use crate::setup::ImportError;
use donder_language::compiler::{DeclarationKind, ParamDecl, ParamRange};
use donder_language::{DonderDuration, DonderTime};
use donder_model::{
    Clip, ClipContent, ClipId, Composition, CurveValue, Effect, FixtureId, GradientValue, LayerId,
    Mark, MarkCollection, MarkCollectionId, ParamValue, Placement, ScriptDefinition, ScriptId,
    Sequence, SequenceAudio, SequenceId,
};
use donder_project_io::ProjectSession;
use donder_runtime_types::{
    Color, Curve, CurvePoint, Gradient, GradientSpace, GradientStop, Identifier, Type,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

/// The layout's fixtures and groups, by name.
fn targets(session: &ProjectSession) -> Vec<(String, FixtureId)> {
    let project = session.project();
    project
        .get::<donder_model::Layout>(project.root().layout)
        .map(|layout| {
            layout
                .fixtures
                .iter()
                .map(|fixture| (fixture.name.as_str().to_owned(), fixture.id))
                .collect()
        })
        .unwrap_or_default()
}

/// The names of the layout's groups and then its fixtures, for the user
/// filling in a mapping.
pub(crate) fn target_names(session: &ProjectSession) -> Vec<String> {
    let project = session.project();
    let Some(layout) = project.get::<donder_model::Layout>(project.root().layout) else {
        return Vec::new();
    };
    let is_group = |fixture: &&donder_model::LayoutFixture| {
        matches!(fixture.kind, donder_model::LayoutFixtureKind::Group { .. })
    };
    let groups = layout.fixtures.iter().filter(is_group);
    let fixtures = layout.fixtures.iter().filter(|fixture| !is_group(fixture));
    groups
        .chain(fixtures)
        .map(|fixture| fixture.name.as_str().to_owned())
        .collect()
}

/// A name made of what Donder allows in identifiers: lower case letters,
/// digits and underscores, starting with a letter.
fn identifier(text: &str, fallback: &str) -> Identifier {
    let mut name = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            name.push(character.to_ascii_lowercase());
        } else if !name.ends_with('_') && !name.is_empty() {
            name.push('_');
        }
    }
    let name = name.trim_end_matches('_');
    let name = if name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        name.to_owned()
    } else {
        format!("{fallback}_{name}")
            .trim_end_matches('_')
            .to_owned()
    };
    Identifier::new(name).unwrap_or_else(|_| unreachable!("sanitized names are identifiers"))
}

/// `base`, or `base_2`, `base_3`, ... when it is taken.
fn unique(base: Identifier, taken: &mut HashSet<String>) -> Identifier {
    let mut name = base.as_str().to_owned();
    let mut n = 1;
    while taken.contains(&name) {
        n += 1;
        name = format!("{}_{n}", base.as_str());
    }
    taken.insert(name.clone());
    Identifier::new(name).unwrap_or_else(|_| unreachable!("numbered identifiers stay identifiers"))
}

/// The likenesses the project declares in [`XLIGHTS_EFFECTS`], by name.
fn library(session: &ProjectSession) -> HashMap<String, ScriptId> {
    session
        .project()
        .reusable::<ScriptDefinition>()
        .iter()
        .filter(|(_, definition)| definition.kind() == DeclarationKind::Effect)
        .filter_map(|(id, definition)| {
            let address = session.address_of(*id);
            (address.document().as_str() == XLIGHTS_EFFECTS)
                .then(|| (definition.name().to_owned(), *id))
        })
        .collect()
}

fn mismatch(effect: &str, param: &str, why: &str) -> ImportError {
    ImportError::Sequence(format!(
        "{XLIGHTS_EFFECTS}'s {effect} {why} `{param}`; use the library the starter project ships."
    ))
}

fn gradient(stops: &[(f32, Color)]) -> Gradient {
    Gradient {
        space: GradientSpace::LinearRgb,
        stops: stops
            .iter()
            .map(|&(position, color)| GradientStop { position, color })
            .collect(),
    }
}

/// The value as the parameter `param` takes it, held inside its range.
fn param_value(effect: &str, param: &ParamDecl, value: &Value) -> Result<ParamValue, ImportError> {
    let name = param.name.as_str();
    let wrong = || mismatch(effect, name, "takes another kind of value for");
    let float_range = match param.range {
        Some(ParamRange::Float { min, max }) => Some((min, max)),
        Some(ParamRange::Int { min, max }) => Some((min as f32, max as f32)),
        None => None,
    };
    Ok(match (&param.ty, value) {
        (Type::Float, Value::Float(value)) => {
            let (min, max) = float_range.unwrap_or((f32::MIN, f32::MAX));
            ParamValue::Float(value.clamp(min, max))
        }
        (Type::Int, Value::Int(value)) => match param.range {
            Some(ParamRange::Int { min, max }) => ParamValue::Int((*value).clamp(min, max)),
            _ => ParamValue::Int(*value),
        },
        (Type::Bool, Value::Bool(value)) => ParamValue::Bool(*value),
        (Type::Enum(options), Value::Option(option)) => {
            let option = options
                .iter()
                .find(|candidate| candidate.as_str() == *option)
                .ok_or_else(|| mismatch(effect, name, &format!("has no option {option} for")))?;
            ParamValue::option(param, option.clone())
        }
        (Type::Gradient, Value::Gradient(stops)) => {
            ParamValue::Gradient(GradientValue::Literal(gradient(stops)))
        }
        (Type::Array(element), Value::Gradients(list)) if **element == Type::Gradient => {
            ParamValue::Array(
                list.iter()
                    .map(|stops| ParamValue::Gradient(GradientValue::Literal(gradient(stops))))
                    .collect(),
            )
        }
        (Type::Curve, Value::Curve(points)) => {
            let (min, max) = float_range.unwrap_or((0.0, 1.0));
            let span = (max - min).max(f32::EPSILON);
            ParamValue::Curve(CurveValue::Literal(Curve {
                points: points
                    .iter()
                    .map(|&(position, value)| CurvePoint {
                        position: position.clamp(0.0, 1.0),
                        value: ((value - min) / span).clamp(0.0, 1.0),
                    })
                    .collect(),
            }))
        }
        _ => return Err(wrong()),
    })
}

/// The xLights time on the sequence's frames.
fn time(at: Duration) -> DonderTime {
    DonderTime(at)
}

pub(crate) fn build(
    session: &mut ProjectSession,
    sequence: &XSequence,
    mapping: &Mapping,
    options: &SequenceImportOptions,
) -> Result<SequenceImportReport, ImportError> {
    let layout = targets(session);
    let by_name = layout.iter().cloned().collect::<HashMap<_, _>>();
    let unknown = mapping
        .rows
        .iter()
        .flat_map(|(_, targets)| targets)
        .filter(|target| !by_name.contains_key(*target))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(ImportError::Sequence(format!(
            "The mapping names targets the layout does not have: {}",
            unknown.join(", ")
        )));
    }
    let library = library(session);
    if library.is_empty() {
        return Err(ImportError::Sequence(format!(
            "The project has no {XLIGHTS_EFFECTS}; copy it from the starter project and import it."
        )));
    }
    let definitions = session.project().reusable::<ScriptDefinition>().clone();

    let mut report = SequenceImportReport::default();
    let mut clips = Vec::new();
    let mut names = HashSet::new();
    for row in &sequence.rows {
        let count = row.effect_count();
        let Some(targets) = mapping.targets(&row.name) else {
            if count > 0 {
                report.unmapped.push((row.name.clone(), count));
            }
            continue;
        };
        let targets = targets
            .iter()
            .filter_map(|target| by_name.get(target).copied())
            .collect::<Vec<_>>();
        for translated in translate::row(row, sequence.frame, &mut report) {
            let effect = translated.port.name();
            let id = library.get(effect).copied().ok_or_else(|| {
                ImportError::Sequence(format!(
                    "{XLIGHTS_EFFECTS} has no effect {effect}; use the library the starter project ships."
                ))
            })?;
            let definition = definitions
                .get(&id)
                .unwrap_or_else(|| unreachable!("the library's effects are the project's"));
            let mut params = Vec::new();
            for (param_name, value) in &translated.params {
                let param = definition
                    .params()
                    .iter()
                    .find(|param| param.name.as_str() == *param_name)
                    .ok_or_else(|| mismatch(effect, param_name, "has no parameter"))?;
                params.push((param.name.clone(), param_value(effect, param, value)?));
            }
            let clip_name = unique(identifier(&translated.port.clip_name(), "clip"), &mut names);
            let duration = translated
                .end
                .min(sequence.duration)
                .saturating_sub(translated.start);
            if duration.is_zero() {
                continue;
            }
            clips.push(Arc::new(Clip {
                id: ClipId(clips.len() as u32),
                name: clip_name,
                description: Some(format!(
                    "xLights {} on {}, layer {}.",
                    translated.source,
                    row.name,
                    translated.layer + 1
                )),
                targets: targets.clone(),
                placement: Placement::Timed {
                    start: time(translated.start),
                    duration: DonderDuration(duration),
                },
                content: ClipContent::Effect(Effect {
                    layer: LayerId(0),
                    scope: translated.scope,
                    definition: id,
                    params: params.into_iter().collect(),
                }),
            }));
        }
    }
    report.clips = clips.len();

    // Each layer of each timing track is a mark collection: for lyrics, the
    // phrases, words and phonemes.
    let mut mark_names = HashSet::new();
    let mut marks = Vec::new();
    for track in &sequence.timings {
        let base = identifier(&track.name, "timing");
        for (index, layer) in track.layers.iter().enumerate() {
            if layer.is_empty() {
                continue;
            }
            let name = if track.layers.len() == 1 {
                unique(base.clone(), &mut mark_names)
            } else {
                unique(
                    identifier(&format!("{}_{}", base.as_str(), index + 1), "timing"),
                    &mut mark_names,
                )
            };
            marks.push(MarkCollection {
                id: MarkCollectionId(marks.len() as u32),
                name,
                description: Some(format!(
                    "xLights timing track {}, layer {}.",
                    track.name,
                    index + 1
                )),
                display_color: Color::from_bytes([0x38, 0xbd, 0xf8]),
                marks: layer
                    .iter()
                    .filter(|label| label.start < sequence.duration)
                    .map(|label| Mark {
                        time: time(label.start),
                        duration: DonderDuration(
                            label.end.min(sequence.duration).saturating_sub(label.start),
                        ),
                        label: (!label.text.trim().is_empty()).then(|| label.text.clone()),
                    })
                    .collect(),
            });
        }
    }
    report.mark_collections = marks.len();

    let mut composition = Composition::default(Color::from_bytes([0x38, 0xbd, 0xf8]));
    composition.marks = marks;
    composition.clips = clips;
    let mut new = Sequence {
        description: Some(format!(
            "Imported from the xLights sequence {}.",
            options
                .sequence
                .file_name()
                .unwrap_or(options.sequence.as_str())
        )),
        duration: DonderDuration(sequence.duration),
        // The nearest whole rate to xLights' frame time: 30 ms is 33 fps.
        frame_rate: ((1.0 / sequence.frame.as_secs_f64()).round() as u32).max(1),
        audio: SequenceAudio::None,
        composition,
    };
    let name = Identifier::new(options.name.clone()).map_err(|_| {
        ImportError::Sequence(format!(
            "`{}` is not a sequence name; use lower case letters, digits and underscores.",
            options.name
        ))
    })?;
    // The sequence and its song go into a copy of the session, which replaces
    // it only when both are in, so a failure leaves the session as it was.
    let mut candidate = session.clone();
    let id = SequenceId(candidate.new_id());
    let document = candidate.edit(|edit| {
        let document = edit.add_object_document(id.into(), name)?;
        edit.project_mut().put((id, new.clone()))?;
        Ok::<_, donder_project_io::SessionEditError>(document)
    })?;
    if let Some(audio) = &options.audio {
        let asset = candidate
            .reference_asset(&document, audio)
            .map_err(|error| ImportError::Sequence(format!("Audio file {audio}: {error}")))?;
        new.audio = SequenceAudio::Asset(asset);
        candidate
            .put((id, new))
            .map_err(|error| ImportError::Sequence(format!("{error:?}")))?;
    }
    *session = candidate;
    Ok(report)
}
