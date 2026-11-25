use crate::effect::{CurveValue, FontFace, GradientValue, ParamValue};
use crate::project::DonderProject;
use crate::sequence::Sequence;
use crate::{AudioAnalysis, AudioFeature, Derivation, Derived, FontRaster, Number};
use donder_language::DonderTime;
use donder_language::compiler::{AudioSpec, CompiledDefinition, ParamDecl, Setting};
use donder_runtime_types::{Font, Image, MarkTrack, Marks, SampleDuration, SampleTime, Track};
use donder_runtime_types::{Identifier, Type, Value};
use indexmap::IndexMap;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) struct ClipTiming {
    pub(crate) start: SampleTime,
    pub(crate) duration: SampleDuration,
}

/// Where marks parameters' collections, audio analyses' audio and mark
/// labels come from.
#[derive(Clone, Copy)]
pub(crate) enum MarkOrigin<'a> {
    /// A sequence's authored mark collections and its audio.
    Authored(&'a Sequence),
    /// A console's live collections, which start empty; playback rebinds them.
    Live,
}

/// Every declared parameter's value, its override or its default, then the
/// fields of each enum option, then the samples of each audio analysis, as
/// the program reads them.
pub(crate) fn prepare_program_values(
    project: &DonderProject,
    marks: MarkOrigin<'_>,
    definition: &CompiledDefinition,
    overrides: &IndexMap<Identifier, ParamValue>,
    timing: ClipTiming,
) -> Vec<Value> {
    let context = Context::new(project, marks, definition, overrides, timing);
    let mut values = declared_program_values(&context, definition.params(), overrides);
    let audio: Vec<Value> = definition
        .audio()
        .iter()
        .map(|spec| {
            let derived = audio_analysis(marks, spec, &values)
                .and_then(|analysis| project.derived(&Derivation::Audio(analysis)));
            Value::Track(Arc::new(match derived {
                Some(Derived::Track(track)) => track.window(timing.start.as_ticks()),
                // No audio, a console, or an analysis running or failed: silence.
                _ => Track::empty(),
            }))
        })
        .collect();
    values.extend(audio);
    values
}

/// The analyses of the sequence's audio, the font rasters and the images
/// `definition` reads with `overrides`.
pub(crate) fn derivations(
    project: &DonderProject,
    origin: MarkOrigin<'_>,
    definition: &CompiledDefinition,
    overrides: &IndexMap<Identifier, ParamValue>,
) -> Vec<Derivation> {
    // Settings are numbers; marks windows do not matter.
    let timing = ClipTiming {
        start: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(0),
    };
    let context = Context::new(project, origin, definition, overrides, timing);
    let mut derivations: Vec<Derivation> = overrides
        .values()
        .filter_map(|value| match value {
            ParamValue::Font(Some(face)) => Some(Derivation::Font(context.raster(face))),
            ParamValue::Image(Some(image)) => Some(Derivation::Image(image.clone())),
            _ => None,
        })
        .collect();
    if !definition.audio().is_empty() {
        let values = declared_program_values(&context, definition.params(), overrides);
        derivations.extend(
            definition
                .audio()
                .iter()
                .filter_map(|spec| audio_analysis(origin, spec, &values))
                .map(Derivation::Audio),
        );
    }
    derivations
}

/// What preparing a program's values reads.
struct Context<'a> {
    project: &'a DonderProject,
    origin: MarkOrigin<'a>,
    timing: ClipTiming,
    /// The characters its fonts draw, by ascending code; empty without a
    /// font parameter.
    characters: Arc<[u32]>,
}

impl<'a> Context<'a> {
    fn new(
        project: &'a DonderProject,
        origin: MarkOrigin<'a>,
        definition: &CompiledDefinition,
        overrides: &IndexMap<Identifier, ParamValue>,
        timing: ClipTiming,
    ) -> Self {
        let draws = definition
            .params()
            .iter()
            .any(|param| param.ty == Type::Font);
        Self {
            project,
            origin,
            timing,
            characters: if draws {
                characters(origin, definition, overrides)
            } else {
                Arc::from([])
            },
        }
    }

    fn raster(&self, face: &FontFace) -> FontRaster {
        FontRaster {
            file: face.file.clone(),
            height: face.height,
            characters: self.characters.clone(),
        }
    }

    /// The same context without collections, for plain values.
    fn plain(&self) -> Self {
        Self {
            project: self.project,
            origin: MarkOrigin::Live,
            timing: self.timing,
            characters: self.characters.clone(),
        }
    }
}

/// The characters a program can draw: printable ASCII, its text literals
/// and parameters, and its sequence's mark labels.
fn characters(
    origin: MarkOrigin<'_>,
    definition: &CompiledDefinition,
    overrides: &IndexMap<Identifier, ParamValue>,
) -> Arc<[u32]> {
    let mut codes: Vec<u32> = (0x20..=0x7e).collect();
    codes.extend(definition.characters());
    for value in overrides.values() {
        if let ParamValue::Text(text) = value {
            codes.extend(text.chars().map(u32::from));
        }
    }
    if let MarkOrigin::Authored(sequence) = origin {
        let labels = sequence
            .composition
            .marks
            .iter()
            .flat_map(|collection| &collection.marks)
            .filter_map(|mark| mark.label.as_deref());
        for label in labels {
            codes.extend(label.chars().map(u32::from));
        }
    }
    codes.sort_unstable();
    codes.dedup();
    codes.into()
}

/// The analysis `spec` makes of the sequence's audio with the program's
/// `values`, or `None` without audio.
fn audio_analysis(
    origin: MarkOrigin<'_>,
    spec: &AudioSpec,
    values: &[Value],
) -> Option<AudioAnalysis> {
    let MarkOrigin::Authored(sequence) = origin else {
        return None;
    };
    let crate::SequenceAudio::Asset(audio) = &sequence.audio else {
        return None;
    };
    let number = |setting: Setting| Number(setting.resolve(values));
    Some(AudioAnalysis {
        audio: audio.clone(),
        feature: match *spec {
            AudioSpec::Level {
                low,
                high,
                attack,
                decay,
            } => AudioFeature::Level {
                low: number(low),
                high: number(high),
                attack: number(attack),
                decay: number(decay),
            },
            AudioSpec::Spectrum {
                bands,
                attack,
                decay,
            } => AudioFeature::Spectrum {
                bands: number(bands),
                attack: number(attack),
                decay: number(decay),
            },
            AudioSpec::Wave => AudioFeature::Wave,
        },
    })
}

/// Every declared parameter's value, then the fields of each enum option.
fn declared_program_values(
    context: &Context<'_>,
    declarations: &[ParamDecl],
    overrides: &IndexMap<Identifier, ParamValue>,
) -> Vec<Value> {
    let values = prepare_params(context, declarations, overrides);
    let declared = declarations
        .iter()
        .map(|param| {
            values
                .get(&param.name)
                .cloned()
                .unwrap_or_else(|| unreachable!("validation proves every parameter has a value"))
        })
        .collect();
    // Field values are plain, so they need no collections or timing.
    let plain = context.plain();
    let chosen: Vec<(usize, Vec<Value>)> = declarations
        .iter()
        .enumerate()
        .filter_map(|(index, param)| match overrides.get(&param.name) {
            Some(ParamValue::Enum { fields, .. }) if !fields.is_empty() => Some((
                index,
                fields
                    .values()
                    .map(|value| prepare_param_value(&plain, value))
                    .collect(),
            )),
            _ => None,
        })
        .collect();
    donder_language::compiler::program_values(declarations, declared, &chosen)
}

fn prepare_params(
    context: &Context<'_>,
    declarations: &[ParamDecl],
    overrides: &IndexMap<Identifier, ParamValue>,
) -> IndexMap<Identifier, Value> {
    let mut params = declarations
        .iter()
        .filter_map(|param| {
            param
                .default
                .as_ref()
                .map(|value| (param.name.clone(), value.clone()))
        })
        .collect::<IndexMap<_, _>>();
    params.extend(
        overrides
            .iter()
            .map(|(key, value)| (key.clone(), prepare_param_value(context, value))),
    );
    params
}

fn prepare_param_value(context: &Context<'_>, value: &ParamValue) -> Value {
    let project = context.project;
    let sequence = match context.origin {
        MarkOrigin::Authored(sequence) => Some(sequence),
        MarkOrigin::Live => None,
    };
    // Loading/edit acceptance establishes reference existence and timing ranges.
    // Here we only turn those authored values into their playback representation.
    match value {
        ParamValue::Int(value) => Value::Int(*value),
        ParamValue::Float(value) => Value::Float(*value),
        ParamValue::Bool(value) => Value::Bool(*value),
        ParamValue::Color(value) => Value::Color(*value),
        ParamValue::Enum { option, .. } => Value::Enum(option.clone()),
        ParamValue::Marks(None) => Value::Marks(Arc::new(Marks::empty())),
        ParamValue::Marks(Some(key)) => {
            let Some(collection) = sequence.and_then(|sequence| {
                sequence
                    .composition
                    .marks
                    .iter()
                    .find(|collection| collection.id == *key)
            }) else {
                // A live collection, which playback rebinds.
                return Value::Marks(Arc::new(Marks::empty()));
            };
            // The collection's whole track; preparation shares equal tracks.
            let mut marks: Vec<(u32, u32, &str)> = collection
                .marks
                .iter()
                .filter_map(|mark| {
                    let start = u32::try_from(mark.time.as_micros_rounded()).ok()?;
                    let end = u32::try_from(DonderTime(mark.end()).as_micros_rounded()).ok()?;
                    Some((start, end - start, mark.label.as_deref().unwrap_or("")))
                })
                .collect();
            marks.sort_unstable();
            let mut label_codes = Vec::new();
            let mut label_spans = Vec::new();
            if marks.iter().any(|(_, _, label)| !label.is_empty()) {
                for (_, _, label) in &marks {
                    let start = label_codes.len() as u32;
                    label_codes.extend(label.chars().map(u32::from));
                    label_spans.extend([start, label_codes.len() as u32 - start]);
                }
            }
            let track = MarkTrack {
                ticks: marks.iter().map(|mark| mark.0).collect::<Vec<_>>().into(),
                durations: marks.iter().map(|mark| mark.1).collect::<Vec<_>>().into(),
                label_codes: label_codes.into(),
                label_spans: label_spans.into(),
            };
            let start = context.timing.start.as_ticks();
            let end = start.saturating_add(context.timing.duration.as_ticks());
            Value::Marks(Arc::new(Marks::window(track, start, end)))
        }
        ParamValue::Curve(source) => Value::Curve(Arc::new(match source {
            CurveValue::Literal(curve) => curve.clone(),
            CurveValue::Linked(id) => project.reusable::<crate::CurveDefinition>()[id]
                .curve
                .clone(),
        })),
        ParamValue::Gradient(source) => Value::Gradient(Arc::new(match source {
            GradientValue::Literal(gradient) => gradient.clone(),
            GradientValue::Linked(id) => project.reusable::<crate::GradientDefinition>()[id]
                .gradient
                .clone(),
        })),
        ParamValue::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| prepare_param_value(context, value))
                .collect::<Vec<_>>()
                .into(),
        ),
        ParamValue::Text(text) => Value::Array(
            text.chars()
                .map(|character| Value::Int(character as i32))
                .collect::<Vec<_>>()
                .into(),
        ),
        ParamValue::Font(face) => {
            let derived = face
                .as_ref()
                .and_then(|face| project.derived(&Derivation::Font(context.raster(face))));
            Value::Font(match derived {
                Some(Derived::Font(font)) => font.clone(),
                // No font, or rasterizing running or failed: blank.
                _ => Arc::new(Font::empty()),
            })
        }
        ParamValue::Image(image) => {
            let derived = image
                .as_ref()
                .and_then(|image| project.derived(&Derivation::Image(image.clone())));
            Value::Image(match derived {
                Some(Derived::Image(image)) => image.clone(),
                // No image, or decoding running or failed: black.
                _ => Arc::new(Image::empty()),
            })
        }
    }
}
