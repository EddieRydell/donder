use camino::Utf8PathBuf;

use crate::dsl::Identifier;

/// The semantic source of an import. Locations and parser-specific spans are
/// deliberately kept outside this declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportSource {
    LocalDocuments { documents: Vec<Utf8PathBuf> },
    DependencyExport { dependency: String, export: String },
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
    Builtin(Identifier),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ImportAlias(Identifier);

impl ImportAlias {
    pub fn new(value: &str) -> Result<Self, String> {
        if !is_valid_import_alias(value) {
            return Err(format!("invalid import alias `{value}`"));
        }
        Identifier::new(value.to_string())
            .map(Self)
            .map_err(|error| format!("{error:?}"))
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

impl SourceReference {
    pub fn parse(value: &str) -> Result<Self, String> {
        let identifier = |value: &str| {
            Identifier::new(value.to_string()).map_err(|_| format!("invalid reference `{value}`"))
        };
        match value.split_once('.') {
            None => identifier(value).map(Self::Local),
            Some(("builtins", name)) => identifier(name).map(Self::Builtin),
            Some((alias, name)) => Ok(Self::Qualified {
                alias: ImportAlias::new(alias)?,
                name: identifier(name)?,
            }),
        }
    }
}

impl std::fmt::Display for SourceReference {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(name) => formatter.write_str(name.as_str()),
            Self::Qualified { alias, name } => write!(formatter, "{alias}.{}", name.as_str()),
            Self::Builtin(name) => write!(formatter, "builtins.{}", name.as_str()),
        }
    }
}

/// Authoring aliases use the same ASCII identifier policy as the DSL.
/// Keywords and the built-in namespace are not valid aliases.
pub fn is_valid_import_alias(value: &str) -> bool {
    use crate::dsl::lexer::{TokenKind, lex};
    if value == "builtins" || Identifier::new(value.to_string()).is_err() {
        return false;
    }
    let tokens = lex(value);
    matches!(tokens.as_slice(), [token, end]
        if token.kind == TokenKind::Identifier && end.kind == TokenKind::Eof)
}
