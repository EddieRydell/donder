//! Object names. Every object other objects refer to has a `snake_case`
//! name, unique where it is declared, and the GUI generates one for each
//! object it creates.
use crate::dsl::Identifier;

/// Whether `text` is a valid object name: `snake_case`, starting with a
/// lowercase letter or `_`.
pub fn is_object_name(text: &str) -> bool {
    crate::data::tree::is_snake_case(text) && Identifier::new(text.to_string()).is_ok()
}

/// The first of `prefix`, `prefix_2`, `prefix_3`, ... that `taken` rejects.
pub fn unique_name(prefix: &str, taken: impl Fn(&str) -> bool) -> Identifier {
    (1_u32..)
        .map(|index| {
            if index == 1 {
                prefix.to_string()
            } else {
                format!("{prefix}_{index}")
            }
        })
        .find(|name| !taken(name))
        .and_then(|name| Identifier::new(name).ok())
        .unwrap_or_else(|| unreachable!("name prefixes are object names"))
}

/// The object name for a display text, like a name typed into the GUI:
/// `Output 01` becomes `output_01`, and text with no letters or digits
/// becomes `object`.
pub fn object_name(text: &str) -> Identifier {
    Identifier::new(name_from_text(text, "object"))
        .unwrap_or_else(|_| unreachable!("derived names are identifiers"))
}

/// A `snake_case` name in `PascalCase`, as enum options are written:
/// `type_1` becomes `Type1`, `per_fixture` becomes `PerFixture`.
pub fn pascal_from_snake(text: &str) -> String {
    text.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + characters.as_str())
                .unwrap_or_default()
        })
        .collect()
}

/// A display text turned into an object name: `Output 01` becomes
/// `output_01`, and `Time Warp` and `TimeWarp` become `time_warp`.
pub fn name_from_text(text: &str, fallback: &str) -> String {
    let mut name = String::new();
    let mut previous_lower = false;
    for character in text.trim().chars() {
        if character.is_ascii_alphanumeric() {
            if name.is_empty() && character.is_ascii_digit() {
                name.push('_');
            }
            if character.is_ascii_uppercase() && previous_lower {
                name.push('_');
            }
            previous_lower = character.is_ascii_lowercase() || character.is_ascii_digit();
            name.push(character.to_ascii_lowercase());
        } else {
            previous_lower = false;
            if !name.is_empty() && !name.ends_with('_') {
                name.push('_');
            }
        }
    }
    let name = name.trim_end_matches('_').to_string();
    if name.is_empty() {
        fallback.to_string()
    } else {
        name
    }
}
