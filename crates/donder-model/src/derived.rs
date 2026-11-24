//! Data derived from asset files that effects read: analyses of a sequence's
//! audio, rasterized fonts and decoded images. The project holds it beside its documents;
//! analysis outside the model delivers it.
use crate::sequence::AssetId;
use std::collections::HashMap;
use std::sync::Arc;

/// What to derive from an asset file; equal derivations share their result.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Derivation {
    Audio(AudioAnalysis),
    Font(FontRaster),
    Image(crate::ImageSource),
}

impl Derivation {
    /// The asset it is derived from.
    pub fn asset(&self) -> &AssetId {
        match self {
            Self::Audio(analysis) => &analysis.audio,
            Self::Font(raster) => &raster.file,
            Self::Image(image) => &image.file,
        }
    }

    /// The same derivation of `asset`.
    pub fn with_asset(&self, asset: AssetId) -> Self {
        let mut derivation = self.clone();
        match &mut derivation {
            Self::Audio(analysis) => analysis.audio = asset,
            Self::Font(raster) => raster.file = asset,
            Self::Image(image) => image.file = asset,
        }
        derivation
    }
}

/// One analysis of an audio file.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AudioAnalysis {
    pub audio: AssetId,
    pub feature: AudioFeature,
}

/// What an analysis measures, as [`donder_language::compiler::AudioSpec`]
/// describes it with each setting resolved.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum AudioFeature {
    Level {
        low: Number,
        high: Number,
        attack: Number,
        decay: Number,
    },
    Spectrum {
        bands: Number,
        attack: Number,
        decay: Number,
    },
    Wave,
}

/// A font file rasterized at `height` pixels for `characters`, by ascending
/// code.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FontRaster {
    pub file: AssetId,
    pub height: u32,
    pub characters: Arc<[u32]>,
}

/// A setting, compared and hashed by its bits.
#[derive(Clone, Copy, Debug)]
pub struct Number(pub f32);

impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for Number {}

impl std::hash::Hash for Number {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

/// A derivation's outcome.
#[derive(Clone, Debug, PartialEq)]
pub enum Derived {
    Track(Arc<donder_runtime_types::Track>),
    Font(Arc<donder_runtime_types::Font>),
    Image(Arc<donder_runtime_types::Image>),
    /// Why the asset could not be read or analyzed.
    Failed(String),
}

/// Each derivation's result. A derivation without an entry is still
/// running: effects read its audio as silence, its font as blank and its
/// image as black.
pub(crate) type DerivedStore = HashMap<Derivation, Derived>;
