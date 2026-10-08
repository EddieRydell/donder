use camino::Utf8PathBuf;

use crate::names::NameKind;
use donder_runtime_types::Identifier;

/// The semantic source of an import. Locations and parser-specific spans are
/// deliberately kept outside this declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportSource {
    LocalDocuments { documents: Vec<Utf8PathBuf> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDeclaration {
    pub alias: ImportAlias,
    pub source: ImportSource,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum SourceReference {
    Local(Identifier),
    Qualified {
        alias: ImportAlias,
        name: Identifier,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ImportAlias(Identifier);

impl ImportAlias {
    /// An alias follows the object-name rule.
    pub fn new(value: &str) -> Result<Self, String> {
        NameKind::Object
            .check(value)
            .map_err(|error| format!("invalid import alias: {}", error.message(value)))?;
        Ok(Self(Identifier::new(value.to_string()).unwrap_or_else(
            |_| unreachable!("object names are identifiers"),
        )))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for ImportAlias {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::fmt::Display for SourceReference {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(name) => formatter.write_str(name.as_str()),
            Self::Qualified { alias, name } => write!(formatter, "{alias}.{}", name.as_str()),
        }
    }
}
