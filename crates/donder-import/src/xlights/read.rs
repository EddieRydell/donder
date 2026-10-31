//! Reading a model's attributes exactly as xLights does: through pugixml's
//! `as_int`, `as_float` and `as_string`, the C library's `strtol`, and the
//! fallbacks xLights' deserializer layers over them. An attribute that is
//! present but empty is present, as it is to pugixml.
//!
//! Every attribute and child element a model has must be accounted for by
//! the tables of its type: read into its settings, known not to change where
//! its lights are or which channels they take, or refused unless it has its
//! default value. Anything else refuses the model, so a setting the importer
//! does not understand never imports wrong.

use crate::report::SkipReason;
use crate::xml::Element;

/// What an attribute or child element is to the importer.
#[derive(Clone, Copy)]
pub(crate) enum Use {
    /// Read into the model's settings.
    Modeled,
    /// Cannot change where the model's lights are or which channels they
    /// take, for the reason given, which documents the table.
    Irrelevant(
        #[expect(dead_code, reason = "the reason documents the table for its readers")]
        &'static str,
    ),
    /// Read, and the model refused unless it has its default value.
    RefusedUnlessDefault,
}

/// An attribute's name: exact, or a prefix followed by a number from 0 or 1
/// up, as xLights writes per-string and per-segment settings.
#[derive(Clone, Copy)]
pub(crate) enum Name {
    Exact(&'static str),
    Numbered(&'static str),
}

impl Name {
    fn matches(self, name: &str) -> bool {
        match self {
            Self::Exact(exact) => name == exact,
            Self::Numbered(prefix) => name.strip_prefix(prefix).is_some_and(|number| {
                !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
            }),
        }
    }
}

pub(crate) type Table = &'static [(Name, Use)];

/// The first attribute of `element` none of `tables` accounts for, then the
/// first such child element.
pub(crate) fn unhandled(
    element: &Element,
    attributes: &[Table],
    children: &[Table],
) -> Result<(), SkipReason> {
    let known = |tables: &[Table], name: &str| {
        tables
            .iter()
            .any(|table| table.iter().any(|(entry, _)| entry.matches(name)))
    };
    if let Some(name) = element
        .attribute_names()
        .find(|name| !known(attributes, name))
    {
        return Err(SkipReason::UnhandledAttribute(name.to_string()));
    }
    if let Some(child) = element
        .children
        .iter()
        .find(|child| !known(children, &child.name))
    {
        return Err(SkipReason::UnhandledElement(child.name.clone()));
    }
    Ok(())
}

/// A model's attributes, read as xLights reads them.
#[derive(Clone, Copy)]
pub(crate) struct Reader<'a> {
    pub(crate) element: &'a Element,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(element: &'a Element) -> Self {
        Self { element }
    }

    /// The attribute's text, when it is present.
    pub(crate) fn raw(&self, name: &str) -> Option<&'a str> {
        self.element.attribute(name)
    }

    /// xLights: `pugi::xml_attribute::empty()`, negated.
    pub(crate) fn has(&self, name: &str) -> bool {
        self.raw(name).is_some()
    }

    /// xLights: `pugi::xml_attribute::as_string(default)`.
    pub(crate) fn as_string(&self, name: &str, default: &'a str) -> &'a str {
        self.raw(name).unwrap_or(default)
    }

    /// xLights: `pugi::xml_attribute::as_int(default)`.
    pub(crate) fn as_int(&self, name: &str, default: i32) -> i32 {
        self.raw(name).map_or(default, pugi_int)
    }

    /// xLights: `pugi::xml_attribute::as_float(default)`.
    pub(crate) fn as_float(&self, name: &str, default: f32) -> Result<f32, SkipReason> {
        match self.raw(name) {
            None => Ok(default),
            Some(text) => strtod(text)
                .map(|value| value as f32)
                .filter(|value| value.is_finite())
                .ok_or_else(|| self.invalid(name)),
        }
    }

    /// xLights: `as_string(default) == expected`, how it reads most flags.
    pub(crate) fn equals(&self, name: &str, default: &str, expected: &str) -> bool {
        self.raw(name).unwrap_or(default) == expected
    }

    /// xLights: `ReadAttrWithParmFallback` in `XmlDeserializingModelFactory`:
    /// the named attribute when present, else the `parm` one, else
    /// `default`, through `strtol` into an `int`.
    pub(crate) fn parm(&self, name: &str, parm: &str, default: &str) -> i32 {
        let text = self.raw(name).or_else(|| self.raw(parm)).unwrap_or(default);
        strtol(text) as i32
    }

    pub(crate) fn invalid(&self, name: &str) -> SkipReason {
        SkipReason::Attribute {
            name: name.to_string(),
            value: self.raw(name).unwrap_or_default().to_string(),
        }
    }
}

/// xLights: `Model::DeserializeLayerSizes`: the positive entries of a
/// comma separated list, in order.
pub(crate) fn layer_sizes(text: &str) -> Vec<i32> {
    text.split(',')
        .map(|item| strtol(item) as i32)
        .filter(|size| *size > 0)
        .collect()
}

/// xLights: `Trim` in `string_utils.h`, which trims spaces and tabs.
pub(crate) fn trim(text: &str) -> &str {
    text.trim_matches([' ', '\t'])
}

/// The C library's `isspace` in the C locale.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// pugixml's `string_to_integer<unsigned int>(value, INT_MIN, INT_MAX)`:
/// leading spaces, a sign, `0x` and hex digits or decimal digits up to the
/// first other character, clamped to the `int` range.
pub(crate) fn pugi_int(text: &str) -> i32 {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() && is_pugi_space(bytes[at]) {
        at += 1;
    }
    let negative = bytes.get(at) == Some(&b'-');
    if matches!(bytes.get(at), Some(b'-' | b'+')) {
        at += 1;
    }
    let hex =
        bytes.get(at) == Some(&b'0') && bytes.get(at + 1).map(|byte| byte | b' ') == Some(b'x');
    let radix: u64 = if hex { 16 } else { 10 };
    if hex {
        at += 2;
    }
    let mut value: u64 = 0;
    let mut overflow = false;
    while let Some(digit) = bytes
        .get(at)
        .and_then(|byte| (*byte as char).to_digit(radix as u32))
    {
        value = value * radix + u64::from(digit);
        if value > u64::from(u32::MAX) {
            overflow = true;
            value = u64::from(u32::MAX);
        }
        at += 1;
    }
    if negative {
        if overflow || value > 1 << 31 {
            i32::MIN
        } else {
            (-(value as i64)) as i32
        }
    } else if overflow || value > i32::MAX as u64 {
        i32::MAX
    } else {
        value as i32
    }
}

/// pugixml's `ct_space`: space, tab, carriage return and line feed.
fn is_pugi_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// The C library's `strtol(text, nullptr, 10)` on a 64-bit `long`.
pub(crate) fn strtol(text: &str) -> i64 {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() && is_space(bytes[at]) {
        at += 1;
    }
    let negative = bytes.get(at) == Some(&b'-');
    if matches!(bytes.get(at), Some(b'-' | b'+')) {
        at += 1;
    }
    let mut value: i64 = 0;
    while let Some(digit) = bytes.get(at).filter(|byte| byte.is_ascii_digit()) {
        let digit = i64::from(digit - b'0');
        value = match value.checked_mul(10).and_then(|value| {
            if negative {
                value.checked_sub(digit)
            } else {
                value.checked_add(digit)
            }
        }) {
            Some(value) => value,
            None => return if negative { i64::MIN } else { i64::MAX },
        };
        at += 1;
    }
    value
}

/// The C library's `strtod`: the longest prefix after leading spaces that
/// is a decimal number, or none when that prefix is a form the importer
/// does not reproduce (hexadecimal, infinity or not-a-number), which
/// refuses the setting.
pub(crate) fn strtod(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let mut start = 0;
    while start < bytes.len() && is_space(bytes[start]) {
        start += 1;
    }
    let mut at = start;
    if matches!(bytes.get(at), Some(b'-' | b'+')) {
        at += 1;
    }
    let rest = &bytes[at..];
    let starts = |prefix: &[u8]| {
        rest.len() >= prefix.len() && rest[..prefix.len()].eq_ignore_ascii_case(prefix)
    };
    if starts(b"0x") || starts(b"inf") || starts(b"nan") {
        return None;
    }
    let digits = |at: &mut usize| {
        let from = *at;
        while bytes.get(*at).is_some_and(u8::is_ascii_digit) {
            *at += 1;
        }
        *at - from
    };
    let mut count = digits(&mut at);
    if bytes.get(at) == Some(&b'.') {
        let mut after = at + 1;
        let fraction = digits(&mut after);
        if count + fraction > 0 {
            at = after;
            count += fraction;
        }
    }
    if count == 0 {
        return Some(0.0);
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        let mut after = at + 1;
        if matches!(bytes.get(after), Some(b'-' | b'+')) {
            after += 1;
        }
        if digits(&mut after) > 0 {
            at = after;
        }
    }
    text[start..at].parse().ok()
}
