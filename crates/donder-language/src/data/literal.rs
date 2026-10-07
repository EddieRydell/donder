//! The one spelling of each literal. Parsing accepts only these spellings, so
//! a saved document keeps every token its author wrote.
use core::time::Duration;

/// An invalid literal: why, and its canonical spelling when it has one.
pub(super) struct Invalid {
    pub(super) message: String,
    pub(super) fix: Option<String>,
}

impl From<String> for Invalid {
    fn from(message: String) -> Self {
        Self { message, fix: None }
    }
}

impl From<&str> for Invalid {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

fn respell(message: String, canonical: String) -> Invalid {
    Invalid {
        message,
        fix: Some(canonical),
    }
}

/// The shortest decimal that reads back as `value`, always with a fraction:
/// `0.6`, `1.0`, `-2.5`. Rust's float `Display` already prints the shortest
/// round-tripping digits, without an exponent.
pub fn canonical_float(value: f32) -> String {
    let text = format!("{value}");
    if text.contains('.') {
        text
    } else {
        format!("{text}.0")
    }
}

/// Seconds with at most nine decimals and no trailing zeros: `1s`, `0.25s`.
pub fn canonical_duration(value: Duration) -> String {
    let nanos = value.subsec_nanos();
    if nanos == 0 {
        format!("{}s", value.as_secs())
    } else {
        let fraction = format!("{nanos:09}");
        format!("{}.{}s", value.as_secs(), fraction.trim_end_matches('0'))
    }
}

/// Meters with at most six decimals and no trailing zeros: `0m`, `0.35m`,
/// `-1.2m`.
pub fn canonical_distance(micrometers: i64) -> String {
    let sign = if micrometers < 0 { "-" } else { "" };
    let magnitude = micrometers.unsigned_abs();
    let (meters, fraction) = (magnitude / 1_000_000, magnitude % 1_000_000);
    if fraction == 0 {
        format!("{sign}{meters}m")
    } else {
        let fraction = format!("{fraction:06}");
        format!("{sign}{meters}.{}m", fraction.trim_end_matches('0'))
    }
}

/// `text` without its `m`, possibly negative, as micrometers.
pub(super) fn distance(text: &str) -> Result<i64, Invalid> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text),
    };
    let (meters, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let invalid = || format!("`{text}m` is not a distance");
    if fraction.len() > 6 {
        return Err("distances are exact to the micrometer: at most six decimals".into());
    }
    let meters = meters.parse::<i64>().map_err(|_| invalid())?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        format!("{fraction:0<6}")
            .parse::<i64>()
            .map_err(|_| invalid())?
    };
    let magnitude = meters
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_add(fraction))
        .ok_or_else(invalid)?;
    let value = if negative { -magnitude } else { magnitude };
    let canonical = canonical_distance(value);
    if canonical == format!("{text}m") {
        Ok(value)
    } else {
        Err(respell(format!("write `{canonical}`"), canonical))
    }
}

/// A float literal's value, or the canonical spelling it should have used.
pub(super) fn float(text: &str) -> Result<f32, Invalid> {
    let value = text
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("`{text}` is not a 32-bit float"))?;
    let canonical = canonical_float(value);
    if canonical == text {
        Ok(value)
    } else {
        Err(respell(
            format!("write `{canonical}`, the shortest spelling of this float"),
            canonical,
        ))
    }
}

pub(super) fn integer(text: &str) -> Result<i64, Invalid> {
    let value = text
        .parse::<i64>()
        .map_err(|_| format!("`{text}` is out of the integer range"))?;
    if value.to_string() == text {
        Ok(value)
    } else {
        Err(respell(
            format!("write `{value}` without leading zeros"),
            value.to_string(),
        ))
    }
}

/// `text` without its `s`.
pub(super) fn duration(text: &str) -> Result<Duration, Invalid> {
    let (seconds, fraction) = text.split_once('.').unwrap_or((text, ""));
    let invalid = || format!("`{text}s` is not a duration");
    let seconds = seconds.parse::<u64>().map_err(|_| invalid())?;
    if fraction.len() > 9 {
        return Err("durations are exact to the nanosecond: at most nine decimals".into());
    }
    let nanos = if fraction.is_empty() {
        0
    } else {
        format!("{fraction:0<9}")
            .parse::<u32>()
            .map_err(|_| invalid())?
    };
    let value = Duration::new(seconds, nanos);
    let canonical = canonical_duration(value);
    if canonical == format!("{text}s") {
        Ok(value)
    } else {
        Err(respell(format!("write `{canonical}`"), canonical))
    }
}

/// A string token's text, quotes included, with its escapes decoded.
pub(crate) fn string(quoted: &str) -> Result<String, String> {
    let mut value = String::new();
    let mut characters = quoted[1..quoted.len() - 1].chars();
    let mut invalid = None;
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some('"') => value.push('"'),
                Some('\\') => value.push('\\'),
                Some('n') => value.push('\n'),
                Some('t') => value.push('\t'),
                other => {
                    invalid = Some(format!(
                        "`\\{}` is not an escape; use `\\\"`, `\\\\`, `\\n` or `\\t`",
                        other.map(String::from).unwrap_or_default()
                    ));
                }
            },
            character if character.is_control() => {
                invalid = Some("write control characters as `\\n` or `\\t`".into());
            }
            character => value.push(character),
        }
    }
    invalid.map_or(Ok(value), Err)
}
