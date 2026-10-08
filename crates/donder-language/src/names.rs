//! Authored names: the one rule for each kind of name, and the names the GUI
//! generates. Every object other objects refer to has a `snake_case` name,
//! unique where it is declared.
use donder_runtime_types::Identifier;

use crate::compiler::builtins::{builtin, is_context_name};
use crate::compiler::syntax::lexer::{LexMode, is_identifier};
use crate::data::tree::{is_pascal_case, is_snake_case};

/// What a name names. Each kind has one rule, which the parsers, the checker,
/// model validation and the language server's rename all apply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameKind {
    /// A data declaration, item or port, or an import alias: `snake_case`
    /// and not a data-document keyword.
    Object,
    /// An effect or operator: `PascalCase`.
    Definition,
    /// An enum option: `PascalCase`.
    EnumOption,
    /// A param or input. Data documents write these as field names and after
    /// `.`, so they are `snake_case` words neither language reserves.
    Member,
    /// An argument, `let` or loop index: `snake_case`, not a context name.
    Value,
    /// A function: a value name that is not a builtin.
    Function,
}

/// Why a name breaks its kind's rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameError {
    NotSnakeCase,
    NotPascalCase,
    /// A keyword, or more than one token.
    Keyword,
    /// A context value, like `time` or `pixel`.
    Context,
    /// A builtin function.
    Builtin,
}

impl NameKind {
    pub fn check(self, text: &str) -> Result<(), NameError> {
        let pascal = matches!(self, Self::Definition | Self::EnumOption);
        if pascal && !is_pascal_case(text) {
            return Err(NameError::NotPascalCase);
        }
        if !pascal && !is_snake_case(text) {
            return Err(NameError::NotSnakeCase);
        }
        let script = self != Self::Object;
        let data = matches!(self, Self::Object | Self::Member);
        if (script && !is_identifier(text, LexMode::Script))
            || (data && !is_identifier(text, LexMode::Data))
        {
            return Err(NameError::Keyword);
        }
        if matches!(self, Self::Member | Self::Value | Self::Function) && is_context_name(text) {
            return Err(NameError::Context);
        }
        if self == Self::Function && builtin(text).is_some() {
            return Err(NameError::Builtin);
        }
        Ok(())
    }

    pub fn accepts(self, text: &str) -> bool {
        self.check(text).is_ok()
    }
}

impl NameError {
    /// What is wrong with `name`, for a diagnostic.
    pub fn message(self, name: &str) -> String {
        match self {
            Self::NotSnakeCase => format!("`{name}` must be snake_case"),
            Self::NotPascalCase => format!("`{name}` must be PascalCase"),
            Self::Keyword => format!("`{name}` is a keyword"),
            Self::Context => format!("`{name}` is a reserved name"),
            Self::Builtin => format!("`{name}` is a builtin"),
        }
    }
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
pub(crate) fn pascal_from_snake(text: &str) -> String {
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
/// `output_01`, and `Time Warp` and `TimeWarp` become `time_warp`. A name
/// that would start with a digit or be a data-document keyword starts with
/// `_`: `None` becomes `_none`.
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
    } else if NameKind::Object.accepts(&name) {
        name
    } else {
        format!("_{name}")
    }
}
