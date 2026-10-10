//! The canonical layout of a data document.
//!
//! - Imports come first, one per line, then a blank line.
//! - A declaration is `Type name {`, one field per line, `}`; declarations
//!   are separated by blank lines.
//! - A record, map, list or tuple stays on one line when that line fits in
//!   [`LINE_WIDTH`] columns. Otherwise each field or item gets its own line,
//!   indented two spaces, with a comma after every one, including the last.
//! - A list of numbers, durations, colors or tuples that does not fit wraps
//!   densely instead: as many items per line as fit.
//!
//! Printing appends to one buffer. A value's fit is decided by writing its
//! one-line form and stopping once the line is full, so each value is measured
//! against at most one line and printing stays linear in the document's size.
use super::LINE_WIDTH;
use super::literal::{canonical_distance, canonical_duration, canonical_float};
use super::tree::*;

pub fn print(document: &DataDocument) -> String {
    print_spliced(document, &[])
}

/// A declaration field printed from items rendered earlier by [`list_item`].
/// The field's value in the document is an empty list. A splice is for a list
/// whose items are each wider than a line, so it never fits on one line.
pub struct ListSplice<'a> {
    /// The declaration's index in the document.
    pub declaration: usize,
    pub field: &'a str,
    pub items: &'a [&'a str],
}

/// [`print`], with the spliced fields written from their rendered items.
pub fn print_spliced(document: &DataDocument, splices: &[ListSplice<'_>]) -> String {
    let spliced = splices
        .iter()
        .flat_map(|splice| splice.items.iter().map(|item| item.len()))
        .sum::<usize>();
    let mut text = String::with_capacity(spliced + 4096);
    for import in &document.imports {
        text.push_str("import ");
        text.push_str(import.alias.value.as_str());
        text.push_str(" from ");
        for (index, path) in import.paths.iter().enumerate() {
            if index > 0 {
                text.push_str(", ");
            }
            text.push('<');
            text.push_str(&path.value);
            text.push('>');
        }
        text.push_str(";\n");
    }
    for (index, declaration) in document.declarations.iter().enumerate() {
        if index > 0 || !document.imports.is_empty() {
            text.push('\n');
        }
        text.push_str(declaration.ty.value.as_str());
        text.push(' ');
        text.push_str(declaration.name.value.as_str());
        text.push_str(" {\n");
        for field in &declaration.fields.value {
            match splices.iter().find(|splice| {
                splice.declaration == index && splice.field == field.name.value.as_str()
            }) {
                Some(splice) if !splice.items.is_empty() => {
                    indent(&mut text, 1);
                    text.push_str(splice.field);
                    text.push_str(": [\n");
                    for item in splice.items {
                        text.push_str(item);
                    }
                    indent(&mut text, 1);
                    text.push_str("],\n");
                }
                _ => field_line(&mut text, field, 1),
            }
        }
        text.push_str("}\n");
    }
    text
}

/// `value` as an item of an expanded list at `level`, as [`print`] writes it:
/// its indentation, the value and the trailing comma and newline.
pub fn list_item(value: &DataValue, level: usize) -> String {
    let mut text = String::new();
    list_item_into(&mut text, value, level);
    text
}

fn list_item_into(text: &mut String, value: &DataValue, level: usize) {
    indent(text, level);
    render(text, value, level, 2 * level, 1);
    text.push_str(",\n");
}

fn indent(text: &mut String, level: usize) {
    for _ in 0..level {
        text.push_str("  ");
    }
}

/// `name: value,` at `level`, expanding the value if it does not fit.
fn field_line(text: &mut String, field: &DataField, level: usize) {
    let line_start = text.len();
    indent(text, level);
    text.push_str(field.name.value.as_str());
    text.push_str(": ");
    let start = text.len() - line_start;
    render(text, &field.value.value, level, start, 1);
    text.push_str(",\n");
}

/// Append `value` beginning at column `start` on a line at `level` and
/// followed by `suffix` columns; multi-line results continue at `level`.
fn render(text: &mut String, value: &DataValue, level: usize, start: usize, suffix: usize) {
    let mark = text.len();
    if !expandable(value) {
        flat(text, value, usize::MAX);
        return;
    }
    if flat(
        text,
        value,
        mark + LINE_WIDTH.saturating_sub(start + suffix),
    ) {
        return;
    }
    text.truncate(mark);
    match value {
        DataValue::Record(ty, fields) => {
            text.push_str(ty.value.as_str());
            text.push_str(" {\n");
            expanded_fields(text, &fields.value, level);
        }
        DataValue::Named(ty, name, fields) => {
            text.push_str(ty.value.as_str());
            text.push(' ');
            text.push_str(name.value.as_str());
            text.push_str(" {\n");
            expanded_fields(text, &fields.value, level);
        }
        DataValue::Map(fields) => {
            text.push_str("{\n");
            expanded_fields(text, fields, level);
        }
        DataValue::List(items) if items.iter().all(|item| dense(&item.value)) => {
            let inner = 2 * (level + 1);
            text.push_str("[\n");
            let mut line = String::new();
            let mut item_text = String::new();
            for item in items {
                item_text.clear();
                flat(&mut item_text, &item.value, usize::MAX);
                item_text.push(',');
                if !line.is_empty() && inner + line.len() + 1 + item_text.len() > LINE_WIDTH {
                    indent(text, level + 1);
                    text.push_str(&line);
                    text.push('\n');
                    line.clear();
                }
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(&item_text);
            }
            if !line.is_empty() {
                indent(text, level + 1);
                text.push_str(&line);
                text.push('\n');
            }
            indent(text, level);
            text.push(']');
        }
        DataValue::List(items) | DataValue::Tuple(items) => {
            let (open, end) = if matches!(value, DataValue::List(_)) {
                ('[', ']')
            } else {
                ('(', ')')
            };
            text.push(open);
            text.push('\n');
            for item in items {
                list_item_into(text, &item.value, level + 1);
            }
            indent(text, level);
            text.push(end);
        }
        _ => unreachable!("only expandable values have an expanded form"),
    }
}

fn expanded_fields(text: &mut String, fields: &[DataField], level: usize) {
    for field in fields {
        field_line(text, field, level + 1);
    }
    indent(text, level);
    text.push('}');
}

fn expandable(value: &DataValue) -> bool {
    match value {
        DataValue::Record(_, fields) | DataValue::Named(_, _, fields) => !fields.value.is_empty(),
        DataValue::Map(fields) => !fields.is_empty(),
        DataValue::List(items) | DataValue::Tuple(items) => !items.is_empty(),
        _ => false,
    }
}

/// Items that pack several to a line.
fn dense(value: &DataValue) -> bool {
    match value {
        DataValue::Integer(_)
        | DataValue::Float(_)
        | DataValue::Duration(_)
        | DataValue::Distance(_)
        | DataValue::Color(_)
        | DataValue::Bool(_) => true,
        DataValue::Tuple(items) => items.iter().all(|item| dense(&item.value)),
        _ => false,
    }
}

/// Append the one-line form of `value`. Returns `false`, leaving a partial
/// form behind, as soon as `text` grows past `limit` bytes.
fn flat(text: &mut String, value: &DataValue, limit: usize) -> bool {
    match value {
        DataValue::Integer(value) => text.push_str(&value.to_string()),
        DataValue::Float(value) => text.push_str(&canonical_float(*value)),
        DataValue::Duration(value) => text.push_str(&canonical_duration(*value)),
        DataValue::Distance(value) => text.push_str(&canonical_distance(*value)),
        DataValue::Color(value) => text.push_str(&value.to_hex()),
        DataValue::String(value) => string(text, value),
        DataValue::Path(value) => {
            text.push('<');
            text.push_str(value);
            text.push('>');
        }
        DataValue::Bool(value) => text.push_str(if *value { "true" } else { "false" }),
        DataValue::None => text.push_str("none"),
        DataValue::Reference(segments) => {
            for (index, segment) in segments.iter().enumerate() {
                if index > 0 {
                    text.push('.');
                }
                text.push_str(segment.value.as_str());
            }
        }
        DataValue::Variant(name) => text.push_str(name.value.as_str()),
        DataValue::Record(ty, body) if body.value.is_empty() => {
            text.push_str(ty.value.as_str());
            text.push_str(" {}");
        }
        DataValue::Record(ty, body) => {
            text.push_str(ty.value.as_str());
            text.push_str(" { ");
            if !flat_fields(text, &body.value, limit) {
                return false;
            }
            text.push_str(" }");
        }
        DataValue::Named(ty, name, body) => {
            text.push_str(ty.value.as_str());
            text.push(' ');
            text.push_str(name.value.as_str());
            text.push_str(" { ");
            if !flat_fields(text, &body.value, limit) {
                return false;
            }
            text.push_str(" }");
        }
        DataValue::Map(body) if body.is_empty() => text.push_str("{}"),
        DataValue::Map(body) => {
            text.push_str("{ ");
            if !flat_fields(text, body, limit) {
                return false;
            }
            text.push_str(" }");
        }
        DataValue::List(items) => {
            text.push('[');
            if !flat_items(text, items, limit) {
                return false;
            }
            text.push(']');
        }
        DataValue::Tuple(items) => {
            text.push('(');
            if !flat_items(text, items, limit) {
                return false;
            }
            text.push(')');
        }
        DataValue::Error => unreachable!("documents with syntax errors are not printed"),
    }
    text.len() <= limit
}

fn flat_items(text: &mut String, items: &[Spanned<DataValue>], limit: usize) -> bool {
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        if !flat(text, &item.value, limit) {
            return false;
        }
    }
    true
}

fn flat_fields(text: &mut String, fields: &[DataField], limit: usize) -> bool {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        text.push_str(field.name.value.as_str());
        text.push_str(": ");
        if !flat(text, &field.value.value, limit) {
            return false;
        }
    }
    true
}

fn string(text: &mut String, value: &str) {
    text.push('"');
    for character in value.chars() {
        match character {
            '"' => text.push_str("\\\""),
            '\\' => text.push_str("\\\\"),
            '\n' => text.push_str("\\n"),
            '\t' => text.push_str("\\t"),
            character => text.push(character),
        }
    }
    text.push('"');
}
