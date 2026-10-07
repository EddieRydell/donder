//! The schema-free syntax tree of a data document. Spans locate every part for
//! diagnostics; equality ignores them, so two trees are equal when they say
//! the same thing.
use crate::dsl::Identifier;
use crate::dsl::TextSpan;
use crate::values::Color;
use core::time::Duration;

#[derive(Clone, Debug)]
pub struct Spanned<T> {
    pub value: T,
    pub span: TextSpan,
}

impl<T> Spanned<T> {
    pub fn new(value: T, span: TextSpan) -> Self {
        Self { value, span }
    }
}

impl<T: PartialEq> PartialEq for Spanned<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DataDocument {
    pub imports: Vec<DataImport>,
    pub declarations: Vec<DataDeclaration>,
}

/// `import alias from <path>, <path>;`
#[derive(Clone, Debug, PartialEq)]
pub struct DataImport {
    pub alias: Spanned<Identifier>,
    pub paths: Vec<Spanned<String>>,
}

/// `Type name { fields }`
#[derive(Clone, Debug, PartialEq)]
pub struct DataDeclaration {
    pub ty: Spanned<Identifier>,
    pub name: Spanned<Identifier>,
    pub fields: Spanned<Vec<DataField>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataField {
    pub name: Spanned<Identifier>,
    pub value: Spanned<DataValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DataValue {
    Integer(i64),
    Float(f32),
    Duration(Duration),
    /// Meters, exact to the micrometer.
    Distance(i64),
    Color(Color),
    String(String),
    /// The path inside `<` and `>`.
    Path(String),
    Bool(bool),
    None,
    /// `name` or `alias.name.child`.
    Reference(Vec<Spanned<Identifier>>),
    /// A fieldless variant: `Multicast`.
    Variant(Spanned<Identifier>),
    /// `Type { fields }`, a record or a variant with fields.
    Record(Spanned<Identifier>, Spanned<Vec<DataField>>),
    /// `Type name { fields }`: an owned object written in place, in the same
    /// form as a declaration.
    Named(
        Spanned<Identifier>,
        Spanned<Identifier>,
        Spanned<Vec<DataField>>,
    ),
    /// `{ name: value }`: parameter values keyed by parameter name.
    Map(Vec<DataField>),
    List(Vec<Spanned<DataValue>>),
    /// `(a, b, ...)`, at least two items.
    Tuple(Vec<Spanned<DataValue>>),
    /// What a syntax error left behind; never printed.
    Error,
}

/// `snake_case`: names of objects, fields and parameters.
pub fn is_snake_case(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first == '_')
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

/// `PascalCase`: types, variants and enum options.
pub fn is_pascal_case(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_uppercase())
        && characters.all(|character| character.is_ascii_alphanumeric())
}
