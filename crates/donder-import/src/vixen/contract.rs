//! Values as .NET's `DataContractSerializer` writes them: each field an
//! element named after it, holding its value as text, with `i:nil="true"`
//! for a null and `i:type` naming a subclass.

use crate::xml::Element;

pub(crate) use crate::report::FieldError;

fn invalid(element: &Element, name: &str) -> FieldError {
    FieldError {
        field: name.to_string(),
        value: element
            .child(name)
            .map(|field| field.text.clone())
            .unwrap_or_default(),
    }
}

pub(crate) fn is_nil(element: &Element) -> bool {
    element.attribute("i:nil") == Some("true")
}

/// The field's element, when it is written and not null.
pub(crate) fn field<'a>(element: &'a Element, name: &str) -> Option<&'a Element> {
    element.child(name).filter(|field| !is_nil(field))
}

/// The field's element, which must be written and not null.
pub(crate) fn required<'a>(element: &'a Element, name: &str) -> Result<&'a Element, FieldError> {
    field(element, name).ok_or_else(|| invalid(element, name))
}

/// The subclass an element holds, by its local name: `a:PreviewLine` is
/// `PreviewLine`.
pub(crate) fn type_name(element: &Element) -> Option<&str> {
    let name = element.attribute("i:type")?;
    Some(name.rsplit_once(':').map_or(name, |(_, local)| local))
}

pub(crate) fn text<'a>(element: &'a Element, name: &str) -> Option<&'a str> {
    field(element, name).map(|field| field.text.as_str())
}

fn parsed<T: std::str::FromStr>(
    element: &Element,
    name: &str,
    default: T,
) -> Result<T, FieldError> {
    match field(element, name) {
        None => Ok(default),
        Some(field) => field
            .text
            .trim()
            .parse()
            .map_err(|_| invalid(element, name)),
    }
}

/// An `int` field; a field not written has its default, as
/// `EmitDefaultValue = false` leaves out a field holding it.
pub(crate) fn int(element: &Element, name: &str, default: i32) -> Result<i32, FieldError> {
    parsed(element, name, default)
}

/// A `double` field.
pub(crate) fn double(element: &Element, name: &str, default: f64) -> Result<f64, FieldError> {
    let value = parsed(element, name, default)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid(element, name))
    }
}

/// A `bool` field, written `true` or `false`.
pub(crate) fn boolean(element: &Element, name: &str, default: bool) -> Result<bool, FieldError> {
    match text(element, name).map(str::trim) {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => Err(invalid(element, name)),
    }
}

/// A `Guid` field, lower case as .NET writes it; the empty GUID is none.
pub(crate) fn guid(element: &Element, name: &str) -> Result<Option<String>, FieldError> {
    match text(element, name).map(str::trim) {
        None => Ok(None),
        Some(text) if is_guid(text) => {
            Ok((text != "00000000-0000-0000-0000-000000000000").then(|| text.to_ascii_lowercase()))
        }
        Some(_) => Err(invalid(element, name)),
    }
}

pub(crate) fn is_guid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

/// A `System.Drawing.Point` or `PreviewPoint` field, `X` and `Y` ints.
pub(crate) fn point(element: &Element, name: &str) -> Result<(i32, i32), FieldError> {
    let point = required(element, name)?;
    Ok((int(point, "X", 0)?, int(point, "Y", 0)?))
}

/// A `List<PreviewPoint>` field.
pub(crate) fn points(element: &Element, name: &str) -> Result<Vec<(i32, i32)>, FieldError> {
    let list = required(element, name)?;
    list.children
        .iter()
        .map(|point| Ok((int(point, "X", 0)?, int(point, "Y", 0)?)))
        .collect()
}
