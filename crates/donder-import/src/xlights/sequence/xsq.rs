//! An xLights sequence file (`.xsq`) read into its rows of effects, its
//! timing tracks and the settings each effect refers to, as xLights'
//! `SequenceElements::LoadSequencerFile` reads them.

use super::settings::{Palette, Settings};
use crate::setup::ImportError;
use crate::xml::{self, Element};
use camino::Utf8Path;
use std::io::Read as _;
use std::rc::Rc;
use std::time::Duration;

/// The effects xLights names in a sequence that the importer tells apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum EffectName {
    On,
    Off,
    Shockwave,
    SingleStrand,
    Pinwheel,
    ColorWash,
    Galaxy,
    Wave,
    Shader,
    Warp,
    MovingHead,
    Faces,
    /// Any effect the importer has no likeness of.
    Other,
}

impl EffectName {
    fn parse(name: &str) -> Self {
        match name {
            "On" => Self::On,
            "Off" => Self::Off,
            "Shockwave" => Self::Shockwave,
            "SingleStrand" => Self::SingleStrand,
            "Pinwheel" => Self::Pinwheel,
            "Color Wash" => Self::ColorWash,
            "Galaxy" => Self::Galaxy,
            "Wave" => Self::Wave,
            "Shader" => Self::Shader,
            "Warp" => Self::Warp,
            "Moving Head" => Self::MovingHead,
            "Faces" => Self::Faces,
            _ => Self::Other,
        }
    }
}

/// One effect placed on a row's layer.
pub(crate) struct Placed {
    pub(crate) name: EffectName,
    /// The name xLights gives it, for the report.
    pub(crate) source_name: String,
    pub(crate) start: Duration,
    pub(crate) end: Duration,
    pub(crate) settings: Rc<Settings>,
    pub(crate) palette: Rc<Palette>,
}

/// A model, group or submodel row: its effect layers, the top one first, as
/// xLights draws them.
pub(crate) struct Row {
    pub(crate) name: String,
    pub(crate) layers: Vec<Vec<Placed>>,
}

impl Row {
    pub(crate) fn effect_count(&self) -> usize {
        self.layers.iter().map(Vec::len).sum()
    }
}

/// A timed label of a timing track.
pub(crate) struct Label {
    pub(crate) start: Duration,
    pub(crate) end: Duration,
    pub(crate) text: String,
}

/// A timing track's layers: for lyrics, the phrases, then the words, then the
/// phonemes.
pub(crate) struct TimingTrack {
    pub(crate) name: String,
    pub(crate) layers: Vec<Vec<Label>>,
}

pub(crate) struct XSequence {
    pub(crate) duration: Duration,
    /// The time between frames xLights renders at.
    pub(crate) frame: Duration,
    pub(crate) rows: Vec<Row>,
    pub(crate) timings: Vec<TimingTrack>,
}

/// The file's time in milliseconds, as xLights writes `startTime` and
/// `endTime`.
fn millis(element: &Element, name: &str) -> Option<Duration> {
    let value = element.attribute(name)?.trim().parse::<f64>().ok()?;
    (value.is_finite() && value >= 0.0).then(|| Duration::from_secs_f64(value / 1000.0))
}

/// A duration xLights writes in seconds, like `173.366`.
fn seconds(text: &str) -> Option<Duration> {
    let value = text.trim().parse::<f64>().ok()?;
    (value.is_finite() && value > 0.0).then(|| Duration::from_secs_f64(value))
}

/// `sequenceTiming`: milliseconds, like `25 ms`.
fn frame_time(text: &str) -> Option<Duration> {
    let value = text
        .trim()
        .trim_end_matches("ms")
        .trim()
        .parse::<u64>()
        .ok()?;
    (value > 0).then(|| Duration::from_millis(value))
}

/// The sequence's XML: the `.xsq` file itself, or the one inside an `.xsqz`
/// package, which xLights packs with the show's layout and shaders.
fn root(path: &Utf8Path) -> Result<Element, ImportError> {
    if path.extension() != Some("xsqz") {
        return xml::read_file(path);
    }
    let error = |message: String| ImportError::Read {
        path: path.to_owned(),
        message,
    };
    let file = fs_err::File::open(path).map_err(|e| error(e.to_string()))?;
    let mut package = zip::ZipArchive::new(file).map_err(|e| error(e.to_string()))?;
    // macOS adds `__MACOSX/._name` copies holding only file metadata.
    let sequences = package
        .file_names()
        .filter_map(Result::ok)
        .filter(|name| name.ends_with(".xsq") && !name.starts_with("__MACOSX/"))
        .map(|name| name.into_owned())
        .collect::<Vec<_>>();
    let [name] = sequences.as_slice() else {
        return Err(error(format!(
            "the package holds {} sequences (.xsq files) instead of one",
            sequences.len()
        )));
    };
    let mut text = String::new();
    package
        .by_name(name)
        .map_err(|e| error(e.to_string()))?
        .read_to_string(&mut text)
        .map_err(|e| error(format!("{name}: {e}")))?;
    xml::parse(&text).map_err(|message| ImportError::Xml {
        path: path.to_owned(),
        message: format!("{name}: {message}"),
    })
}

pub(crate) fn read(path: &Utf8Path) -> Result<XSequence, ImportError> {
    let root = root(path)?;
    let invalid = |message: &str| ImportError::Xml {
        path: path.to_owned(),
        message: message.to_owned(),
    };
    let head = root
        .child("head")
        .ok_or_else(|| invalid("it has no <head>"))?;
    let duration = head
        .child("sequenceDuration")
        .and_then(|element| seconds(&element.text))
        .ok_or_else(|| invalid("its <sequenceDuration> is not a positive number of seconds"))?;
    let frame = head
        .child("sequenceTiming")
        .and_then(|element| frame_time(&element.text))
        .ok_or_else(|| invalid("its <sequenceTiming> is not a number of milliseconds"))?;
    let settings = root
        .child("EffectDB")
        .map(|db| {
            db.children("Effect")
                .map(|effect| Rc::new(Settings::parse(&effect.text)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let palettes = root
        .child("ColorPalettes")
        .map(|palettes| {
            palettes
                .children("ColorPalette")
                .map(|palette| Rc::new(Palette::parse(&palette.text)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let empty_settings = Rc::new(Settings::default());
    let empty_palette = Rc::new(Palette::parse(""));
    let effects = root
        .child("ElementEffects")
        .ok_or_else(|| invalid("it has no <ElementEffects>"))?;
    let mut rows = Vec::new();
    let mut timings = Vec::new();
    for element in effects.children("Element") {
        let name = element.attribute("name").unwrap_or_default().to_owned();
        match element.attribute("type") {
            Some("timing") => timings.push(TimingTrack {
                name,
                layers: element
                    .children("EffectLayer")
                    .map(|layer| {
                        layer
                            .children("Effect")
                            .filter_map(|label| {
                                Some(Label {
                                    start: millis(label, "startTime")?,
                                    end: millis(label, "endTime")?,
                                    text: label.attribute("label").unwrap_or_default().to_owned(),
                                })
                            })
                            .collect()
                    })
                    .collect(),
            }),
            Some("model") => {
                let mut push = |name: String, owner: &Element| {
                    let layers = owner
                        .children("EffectLayer")
                        .map(|layer| {
                            layer
                                .children("Effect")
                                .filter_map(|effect| {
                                    let source_name = effect.attribute("name")?.to_owned();
                                    let index = |key: &str| {
                                        effect
                                            .attribute(key)
                                            .and_then(|index| index.parse::<usize>().ok())
                                    };
                                    Some(Placed {
                                        name: EffectName::parse(&source_name),
                                        source_name,
                                        start: millis(effect, "startTime")?,
                                        end: millis(effect, "endTime")?,
                                        settings: index("ref")
                                            .and_then(|index| settings.get(index).cloned())
                                            .unwrap_or_else(|| Rc::clone(&empty_settings)),
                                        palette: index("palette")
                                            .and_then(|index| palettes.get(index).cloned())
                                            .unwrap_or_else(|| Rc::clone(&empty_palette)),
                                    })
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>();
                    rows.push(Row { name, layers });
                };
                push(name.clone(), element);
                // Submodels and strands carry their own rows, named under the model.
                for part in element
                    .children("SubModelEffectLayer")
                    .chain(element.children("Strand"))
                {
                    let part_name = part.attribute("name").unwrap_or_default();
                    push(format!("{name}/{part_name}"), part);
                }
            }
            _ => {}
        }
    }
    Ok(XSequence {
        duration,
        frame,
        rows,
        timings,
    })
}
