//! Which fixtures and groups of the project each xLights row plays on: a
//! text file of one row per line, `row name<TAB>target, target, ...`, which
//! the importer starts for the user to fill in. Lines starting with `#` are
//! notes; a row with no targets is left out.

use super::xsq::XSequence;
use crate::setup::ImportError;
use camino::Utf8Path;
use std::fmt::Write as _;

pub(crate) struct Mapping {
    /// Each mapped row and the names of the fixtures and groups it plays on.
    pub(crate) rows: Vec<(String, Vec<String>)>,
}

impl Mapping {
    pub(crate) fn read(path: &Utf8Path) -> Result<Self, ImportError> {
        let text = fs_err::read_to_string(path).map_err(|error| ImportError::Read {
            path: path.to_owned(),
            message: error.to_string(),
        })?;
        let mut rows = Vec::new();
        for (number, line) in text.lines().enumerate() {
            if line.trim_start().starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let (row, targets) = line.split_once('\t').ok_or_else(|| {
                ImportError::Sequence(format!(
                    "{path}, line {}: put a tab between the row's name and its targets.",
                    number + 1
                ))
            })?;
            let targets = targets
                .split(',')
                .map(str::trim)
                .filter(|target| !target.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let row = row.trim().to_owned();
            if rows.iter().any(|(name, _)| *name == row) {
                return Err(ImportError::Sequence(format!(
                    "{path}, line {}: the row {row} is mapped twice.",
                    number + 1
                )));
            }
            if !targets.is_empty() {
                rows.push((row, targets));
            }
        }
        Ok(Self { rows })
    }

    pub(crate) fn targets(&self, row: &str) -> Option<&[String]> {
        self.rows
            .iter()
            .find(|(name, _)| name == row.trim())
            .map(|(_, targets)| targets.as_slice())
    }
}

/// A mapping with every row that has effects and no targets yet, each with
/// its effects counted, for the user to fill in.
pub(crate) fn starter(sequence: &XSequence, targets: &[String]) -> String {
    let mut text = String::from(
        "# Which fixtures and groups each xLights row plays on.\n\
         # After each row name, a tab, then the targets, comma separated; a row\n\
         # with none is left out.\n",
    );
    let _ = writeln!(text, "# Targets: {}", targets.join(", "));
    for row in &sequence.rows {
        let count = row.effect_count();
        if count == 0 {
            continue;
        }
        let _ = writeln!(text, "\n# {count} effects on {} layers", row.layers.len());
        let _ = writeln!(text, "{}\t", row.name);
    }
    text
}
