//! An xLights sequence played on a Donder project's fixtures: each xLights
//! row's effects become clips of the likenesses in the project's
//! `effects/xlights.donder`, on the fixtures and groups a mapping names.
//!
//! xLights draws a row's layers into one buffer, mixing each into the layers
//! under it. The importer keeps the mixes the sequence leans on and plays the
//! rest as Donder layers do, by the brightest channel: an `On` or `Color Wash`
//! that unmasks the layers under it colors their clips instead, and every
//! other mix is reported as played by the brightest channel.

mod compose;
mod mapping;
mod settings;
mod translate;
mod xsq;

use crate::setup::ImportError;
use camino::{Utf8Path, Utf8PathBuf};
use donder_project_io::ProjectSession;
pub(crate) use mapping::Mapping;
use std::collections::BTreeMap;
use std::fmt;

/// The effect library the likenesses come from, relative to the project root.
pub const XLIGHTS_EFFECTS: &str = "effects/xlights.donder";

#[derive(Clone, Debug)]
pub struct SequenceImportOptions {
    /// The `.xsq` file, or an `.xsqz` package holding one.
    pub sequence: Utf8PathBuf,
    /// The mapping of xLights rows to fixtures and groups; written for the
    /// user to fill in when it does not exist.
    pub mapping: Utf8PathBuf,
    /// The new sequence's name.
    pub name: String,
    /// The song, relative to the project root.
    pub audio: Option<String>,
}

/// What an import did.
pub enum SequenceImport {
    /// The mapping did not exist; one was written for the user to fill in.
    MappingStarted(Utf8PathBuf),
    Imported(SequenceImportReport),
}

/// What became of the sequence's effects.
#[derive(Clone, Debug, Default)]
pub struct SequenceImportReport {
    pub clips: usize,
    pub mark_collections: usize,
    /// Effects played, by xLights effect name.
    pub played: BTreeMap<String, usize>,
    /// Effects that color the clips under them, by xLights effect name.
    pub tinting: BTreeMap<String, usize>,
    /// Effects left out, by xLights effect name and why.
    pub left_out: BTreeMap<(String, LeftOut), usize>,
    /// Layer mixes played by the brightest channel instead, by mix name.
    pub mixed_by_max: BTreeMap<String, usize>,
    /// Shader files played by a general likeness, by file name.
    pub general_shaders: BTreeMap<String, usize>,
    /// Rows with effects the mapping does not place, and their effect counts.
    pub unmapped: Vec<(String, usize)>,
}

/// Why an effect is not played.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LeftOut {
    /// Draws nothing.
    Off,
    /// Unmasks the layers under it, and none plays there.
    NothingToUnmask,
    /// Needs fixtures Donder has no likeness for (moving heads, singing faces).
    NoLikeness,
    /// Distorts the layers under it.
    Distortion,
}

impl fmt::Display for LeftOut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Off => "draws nothing",
            Self::NothingToUnmask => "unmasks layers with nothing playing under it",
            Self::NoLikeness => "has no likeness in Donder",
            Self::Distortion => "distorts the layers under it, which Donder does not do",
        })
    }
}

impl fmt::Display for SequenceImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Imported {} clips and {} mark collections.",
            self.clips, self.mark_collections
        )?;
        let total = |map: &BTreeMap<String, usize>| map.values().sum::<usize>();
        writeln!(f, "Played ({}):", total(&self.played))?;
        for (name, count) in &self.played {
            writeln!(f, "  {name}: {count}")?;
        }
        if !self.tinting.is_empty() {
            writeln!(f, "Coloring the clips under them:")?;
            for (name, count) in &self.tinting {
                writeln!(f, "  {name}: {count}")?;
            }
        }
        if !self.left_out.is_empty() {
            writeln!(f, "Left out:")?;
            for ((name, reason), count) in &self.left_out {
                writeln!(f, "  {name}: {count}, {reason}")?;
            }
        }
        if !self.mixed_by_max.is_empty() {
            writeln!(f, "Layer mixes played by the brightest channel:")?;
            for (mix, count) in &self.mixed_by_max {
                writeln!(f, "  {mix}: {count}")?;
            }
        }
        if !self.general_shaders.is_empty() {
            writeln!(
                f,
                "Shaders with no likeness of their own, played as a general one:"
            )?;
            for (file, count) in &self.general_shaders {
                writeln!(f, "  {file}: {count}")?;
            }
        }
        if !self.unmapped.is_empty() {
            writeln!(f, "Rows the mapping does not place:")?;
            for (row, count) in &self.unmapped {
                writeln!(f, "  {row}: {count} effects")?;
            }
        }
        Ok(())
    }
}

/// Import the xLights sequence into the project as a new sequence, or start
/// its mapping when there is none yet.
pub fn import_sequence(
    session: &mut ProjectSession,
    options: &SequenceImportOptions,
) -> Result<SequenceImport, ImportError> {
    let sequence = xsq::read(&options.sequence)?;
    if !options.mapping.is_file() {
        let targets = compose::target_names(session);
        write(&options.mapping, &mapping::starter(&sequence, &targets))?;
        return Ok(SequenceImport::MappingStarted(options.mapping.clone()));
    }
    let mapping = Mapping::read(&options.mapping)?;
    let report = compose::build(session, &sequence, &mapping, options)?;
    Ok(SequenceImport::Imported(report))
}

fn write(path: &Utf8Path, text: &str) -> Result<(), ImportError> {
    fs_err::write(path, text).map_err(|error| ImportError::Write {
        path: path.to_owned(),
        message: error.to_string(),
    })
}
