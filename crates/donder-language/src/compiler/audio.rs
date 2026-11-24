//! Analyses of the sequence's audio that effects read. Each is fixed per
//! clip: preparation analyzes the audio once for each distinct analysis and
//! binds its samples to a hidden parameter, which the program reads.
use donder_runtime_types::Value;

/// One analysis a definition reads, with settings fixed per clip.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioSpec {
    /// How loud the audio is between `low` and `high` hertz, 0..1 of its
    /// loudest, following rising sound over `attack` seconds and falling
    /// sound over `decay`.
    Level {
        low: Setting,
        high: Setting,
        attack: Setting,
        decay: Setting,
    },
    /// The loudness of `bands` bands spaced evenly in pitch across the
    /// audible range, in decibels below the loudest, 0..1.
    Spectrum {
        bands: Setting,
        attack: Setting,
        decay: Setting,
    },
    /// The signal itself, -1..1 of its loudest.
    Wave,
}

/// A setting of an analysis: a number, or the value of the program's
/// parameter at an index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Setting {
    Number(f32),
    Param(usize),
}

impl Setting {
    /// The setting with the program's `values`.
    pub fn resolve(self, values: &[Value]) -> f32 {
        match self {
            Self::Number(number) => number,
            Self::Param(index) => match values.get(index) {
                Some(Value::Float(value)) => *value,
                Some(Value::Int(value)) => *value as f32,
                _ => unreachable!("checking proves an audio setting names a number parameter"),
            },
        }
    }

    fn param(self) -> Option<usize> {
        match self {
            Self::Number(_) => None,
            Self::Param(index) => Some(index),
        }
    }
}

impl AudioSpec {
    /// The parameters that set the analysis.
    pub(crate) fn params(&self) -> impl Iterator<Item = usize> {
        let settings: Vec<Setting> = match *self {
            Self::Level {
                low,
                high,
                attack,
                decay,
            } => vec![low, high, attack, decay],
            Self::Spectrum {
                bands,
                attack,
                decay,
            } => vec![bands, attack, decay],
            Self::Wave => Vec::new(),
        };
        settings.into_iter().filter_map(Setting::param)
    }
}

/// Which analysis an audio builtin reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioKind {
    Level,
    Band,
    Wave,
}
