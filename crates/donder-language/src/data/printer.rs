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
use super::LINE_WIDTH;
use super::literal::{canonical_distance, canonical_duration, canonical_float};
use super::tree::*;

pub fn print(document: &DataDocument) -> String {
    let mut text = String::new();
    for import in &document.imports {
        let paths = import
            .paths
            .iter()
            .map(|path| format!("<{}>", path.value))
            .collect::<Vec<_>>()
            .join(", ");
        text.push_str(&format!(
            "import {} from {paths};\n",
            import.alias.value.as_str()
        ));
    }
    for (index, declaration) in document.declarations.iter().enumerate() {
        if index > 0 || !document.imports.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!(
            "{} {} {{\n",
            declaration.ty.value.as_str(),
            declaration.name.value.as_str()
        ));
        for field in &declaration.fields.value {
            field_line(&mut text, field, 1);
        }
        text.push_str("}\n");
    }
    text
}

fn indent(level: usize) -> String {
    "  ".repeat(level)
}

/// `name: value,` at `level`, expanding the value if it does not fit.
fn field_line(text: &mut String, field: &DataField, level: usize) {
    let prefix = format!("{}{}: ", indent(level), field.name.value.as_str());
    let value = render(&field.value.value, level, prefix.len(), 1);
    text.push_str(&prefix);
    text.push_str(&value);
    text.push_str(",\n");
}

/// `value` beginning at column `start` on a line at `level` and followed by
/// `suffix` columns; multi-line results continue at `level`.
fn render(value: &DataValue, level: usize, start: usize, suffix: usize) -> String {
    let one_line = flat(value);
    if start + one_line.len() + suffix <= LINE_WIDTH || !expandable(value) {
        return one_line;
    }
    let inner = indent(level + 1);
    let close = indent(level);
    match value {
        DataValue::Record(ty, fields) => {
            let mut text = format!("{} {{\n", ty.value.as_str());
            for field in &fields.value {
                field_line(&mut text, field, level + 1);
            }
            text.push_str(&close);
            text.push('}');
            text
        }
        DataValue::Named(ty, name, fields) => {
            let mut text = format!("{} {} {{\n", ty.value.as_str(), name.value.as_str());
            for field in &fields.value {
                field_line(&mut text, field, level + 1);
            }
            text.push_str(&close);
            text.push('}');
            text
        }
        DataValue::Map(fields) => {
            let mut text = "{\n".to_string();
            for field in fields {
                field_line(&mut text, field, level + 1);
            }
            text.push_str(&close);
            text.push('}');
            text
        }
        DataValue::List(items) if items.iter().all(|item| dense(&item.value)) => {
            let mut text = "[\n".to_string();
            let mut line = String::new();
            for item in items {
                let item = format!("{},", flat(&item.value));
                if !line.is_empty() && inner.len() + line.len() + 1 + item.len() > LINE_WIDTH {
                    text.push_str(&inner);
                    text.push_str(&line);
                    text.push('\n');
                    line.clear();
                }
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(&item);
            }
            if !line.is_empty() {
                text.push_str(&inner);
                text.push_str(&line);
                text.push('\n');
            }
            text.push_str(&close);
            text.push(']');
            text
        }
        DataValue::List(items) | DataValue::Tuple(items) => {
            let (open, end) = if matches!(value, DataValue::List(_)) {
                ('[', ']')
            } else {
                ('(', ')')
            };
            let mut text = format!("{open}\n");
            for item in items {
                text.push_str(&inner);
                text.push_str(&render(&item.value, level + 1, inner.len(), 1));
                text.push_str(",\n");
            }
            text.push_str(&close);
            text.push(end);
            text
        }
        _ => one_line,
    }
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

/// The one-line form.
fn flat(value: &DataValue) -> String {
    let join = |items: &[Spanned<DataValue>]| {
        items
            .iter()
            .map(|item| flat(&item.value))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let fields = |fields: &[DataField]| {
        fields
            .iter()
            .map(|field| {
                format!(
                    "{}: {}",
                    field.name.value.as_str(),
                    flat(&field.value.value)
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    match value {
        DataValue::Integer(value) => value.to_string(),
        DataValue::Float(value) => canonical_float(*value),
        DataValue::Duration(value) => canonical_duration(*value),
        DataValue::Distance(value) => canonical_distance(*value),
        DataValue::Color(value) => value.to_hex(),
        DataValue::String(value) => string(value),
        DataValue::Path(value) => format!("<{value}>"),
        DataValue::Bool(value) => value.to_string(),
        DataValue::None => "none".into(),
        DataValue::Reference(segments) => segments
            .iter()
            .map(|segment| segment.value.as_str())
            .collect::<Vec<_>>()
            .join("."),
        DataValue::Variant(name) => name.value.as_str().to_string(),
        DataValue::Record(ty, body) if body.value.is_empty() => {
            format!("{} {{}}", ty.value.as_str())
        }
        DataValue::Record(ty, body) => {
            format!("{} {{ {} }}", ty.value.as_str(), fields(&body.value))
        }
        DataValue::Named(ty, name, body) => format!(
            "{} {} {{ {} }}",
            ty.value.as_str(),
            name.value.as_str(),
            fields(&body.value)
        ),
        DataValue::Map(body) if body.is_empty() => "{}".into(),
        DataValue::Map(body) => format!("{{ {} }}", fields(body)),
        DataValue::List(items) => format!("[{}]", join(items)),
        DataValue::Tuple(items) => format!("({})", join(items)),
        DataValue::Error => unreachable!("documents with syntax errors are not printed"),
    }
}

fn string(value: &str) -> String {
    let mut text = String::from('"');
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
    text
}
