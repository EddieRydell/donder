//! Writing the missing fields a data document's diagnostics offer, as when a
//! document predates a field the format now requires.
use donder_language::compiler::Fix;
use donder_language::data::{parse, print};

/// A missing field, `name: value`, and where its document can take it: before
/// the field at byte `at`, or after the value there when `after` is set.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FieldAddition {
    pub at: usize,
    pub field: String,
    pub after: bool,
}

/// `text` with `addition` written, printed canonically. Fails when the
/// document no longer offers that field there.
pub fn add_field(text: &str, addition: &FieldAddition) -> Result<String, String> {
    if !additions(text).contains(addition) {
        return Err(format!(
            "The document no longer needs `{}` there.",
            addition.field
        ));
    }
    let FieldAddition { at, field, after } = addition;
    let written = if *after {
        format!(", {field}")
    } else {
        format!("{field}, ")
    };
    let mut changed = text.to_string();
    changed.insert_str(*at, &written);
    let (document, diagnostics) = parse(&changed);
    match diagnostics.first() {
        Some(diagnostic) => Err(diagnostic.message.clone()),
        None => Ok(print(&document)),
    }
}

/// `text` with every missing field its diagnostics offer written, one at a
/// time, since a record reports only its first missing field.
pub fn add_missing_fields(text: &str) -> Result<String, String> {
    let mut text = text.to_string();
    while let Some(addition) = additions(&text).first() {
        text = add_field(&text, addition)?;
    }
    Ok(text)
}

fn additions(text: &str) -> Vec<FieldAddition> {
    crate::document::read(text)
        .1
        .into_iter()
        .filter_map(|diagnostic| match diagnostic.fix? {
            Fix::AddField { at, field, after } => Some(FieldAddition { at, field, after }),
            Fix::Replace(_) => None,
        })
        .collect()
}
