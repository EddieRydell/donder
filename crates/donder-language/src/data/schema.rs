//! Typed data documents. A type that implements [`Data`] decodes from a
//! syntax tree value and encodes back to one; `#[derive(Data)]` from
//! `donder-data-derive` implements it for records and variants, so a document
//! type's Rust definition is its schema.
//!
//! Decoding is strict: every record field is present and in order, and a
//! value has exactly the form of its type. That is what makes encoding then
//! decoding, and decoding then encoding, both exact.
use super::tree::{DataField, DataValue, Spanned};
use crate::dsl::{Diagnostic, Identifier, TextSpan};
use crate::values::Color;
use core::time::Duration;
use indexmap::IndexMap;

/// Collects diagnostics while decoding; a value that fails to decode reports
/// why and yields `None`.
#[derive(Default)]
pub struct Decoder {
    pub diagnostics: Vec<Diagnostic>,
}

impl Decoder {
    pub fn error(&mut self, span: TextSpan, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::new(span, message));
    }

    /// Report that `value` is not `expected` and fail.
    pub fn mismatch<T>(&mut self, value: &Spanned<DataValue>, expected: &str) -> Option<T> {
        if value.value != DataValue::Error {
            let message = format!("expected {expected}, found {}", describe(&value.value));
            self.error(value.span, message);
        }
        None
    }
}

/// A position for encoded values, which have no source.
pub const NO_SPAN: TextSpan = TextSpan { start: 0, end: 0 };

pub fn spanned(value: DataValue) -> Spanned<DataValue> {
    Spanned::new(value, NO_SPAN)
}

pub trait Data: Sized {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self>;
    fn encode(&self) -> DataValue;
    /// The value's shape, registering named record and variant types.
    fn shape(schema: &mut Schema) -> Shape;
}

/// A record type: its fields decode and encode on their own, as a
/// declaration's body or inside `Type { ... }`.
pub trait Record: Data {
    const TYPE: &'static str;
    fn decode_fields(fields: &Spanned<Vec<DataField>>, decoder: &mut Decoder) -> Option<Self>;
    fn encode_fields(&self) -> Vec<DataField>;
}

/// Decode `Type { fields }` for a record type.
pub fn decode_record<T: Record>(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<T> {
    match &value.value {
        DataValue::Record(ty, fields) if ty.value.as_str() == T::TYPE => {
            T::decode_fields(fields, decoder)
        }
        _ => decoder.mismatch(value, &format!("`{} {{ ... }}`", T::TYPE)),
    }
}

/// Checks a record's fields against its schema in order, one at a time.
pub struct FieldReader<'a> {
    ty: &'static str,
    fields: &'a Spanned<Vec<DataField>>,
    next: usize,
    failed: bool,
    /// A field was missing or out of order; later ones are not reported.
    lost: bool,
    /// A syntax error, already reported, cut the field list short, so missing
    /// and extra fields are not reported again.
    broken: bool,
}

impl<'a> FieldReader<'a> {
    pub fn new(ty: &'static str, fields: &'a Spanned<Vec<DataField>>) -> Self {
        Self {
            ty,
            fields,
            next: 0,
            failed: false,
            lost: false,
            broken: fields
                .value
                .iter()
                .any(|field| matches!(field.value.value, DataValue::Error)),
        }
    }

    /// The next field, which must be `name`.
    pub fn field<T: Data>(&mut self, name: &str, decoder: &mut Decoder) -> Option<T> {
        if self.lost {
            return None;
        }
        let Some(field) = self.fields.value.get(self.next) else {
            if !self.broken {
                decoder.error(
                    self.fields.span,
                    format!("`{}` needs the field `{name}` here", self.ty),
                );
            }
            self.failed = true;
            self.lost = true;
            return None;
        };
        if field.name.value.as_str() != name && self.broken {
            self.failed = true;
            self.lost = true;
            return None;
        }
        if field.name.value.as_str() != name {
            let message = if self.fields.value[self.next..]
                .iter()
                .any(|later| later.name.value.as_str() == name)
            {
                format!(
                    "`{}` lists `{name}` before `{}`",
                    self.ty,
                    field.name.value.as_str()
                )
            } else {
                format!("`{}` needs the field `{name}` here", self.ty)
            };
            decoder.error(field.name.span, message);
            self.failed = true;
            self.lost = true;
            return None;
        }
        self.next += 1;
        let value = T::decode(&field.value, decoder);
        self.failed |= value.is_none();
        value
    }

    /// Report fields left over after the schema's last one.
    pub fn finish(self, decoder: &mut Decoder) -> Option<()> {
        if !self.lost
            && !self.broken
            && let Some(extra) = self.fields.value.get(self.next)
        {
            decoder.error(
                extra.name.span,
                format!("`{}` has no field `{}`", self.ty, extra.name.value.as_str()),
            );
            return None;
        }
        (!self.failed).then_some(())
    }
}

pub fn field(name: &str, value: DataValue) -> DataField {
    DataField {
        name: Spanned::new(identifier(name), NO_SPAN),
        value: spanned(value),
    }
}

pub fn identifier(name: &str) -> Identifier {
    Identifier::new(name.to_string()).unwrap_or_else(|_| unreachable!("schema names are valid"))
}

pub fn record(ty: &str, fields: Vec<DataField>) -> DataValue {
    DataValue::Record(
        Spanned::new(identifier(ty), NO_SPAN),
        Spanned::new(fields, NO_SPAN),
    )
}

/// `Type name { fields }` for a record type's value named `name`.
pub fn declaration<T: Record>(name: &Identifier, value: &T) -> super::tree::DataDeclaration {
    super::tree::DataDeclaration {
        ty: Spanned::new(identifier(T::TYPE), NO_SPAN),
        name: Spanned::new(name.clone(), NO_SPAN),
        fields: Spanned::new(value.encode_fields(), NO_SPAN),
    }
}

pub fn variant(name: &str) -> DataValue {
    DataValue::Variant(Spanned::new(identifier(name), NO_SPAN))
}

/// What a value looks like, for diagnostics.
pub fn describe(value: &DataValue) -> String {
    match value {
        DataValue::Integer(_) => "an integer".into(),
        DataValue::Float(_) => "a float".into(),
        DataValue::Duration(_) => "a duration".into(),
        DataValue::Distance(_) => "a distance".into(),
        DataValue::Color(_) => "a color".into(),
        DataValue::String(_) => "a string".into(),
        DataValue::Path(_) => "a path".into(),
        DataValue::Bool(_) => "a bool".into(),
        DataValue::None => "`none`".into(),
        DataValue::Reference(_) => "a reference".into(),
        DataValue::Variant(name) => format!("`{}`", name.value.as_str()),
        DataValue::Record(ty, _) => format!("`{} {{ ... }}`", ty.value.as_str()),
        DataValue::Named(ty, name, _) => {
            format!("`{} {} {{ ... }}`", ty.value.as_str(), name.value.as_str())
        }
        DataValue::Map(_) => "parameter values".into(),
        DataValue::List(_) => "a list".into(),
        DataValue::Tuple(_) => "a tuple".into(),
        DataValue::Error => "an error".into(),
    }
}

/// The shape of a value in the schema.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Integer,
    Float,
    Duration,
    Distance,
    Color,
    String,
    Path,
    Bool,
    /// An object's own `snake_case` name.
    Name,
    /// A reference to another object.
    Reference,
    /// Parameter values, typed by a definition.
    Params,
    Option(Box<Shape>),
    List(Box<Shape>),
    Tuple(Vec<Shape>),
    /// A record or variant type registered in the [`Schema`].
    Named(&'static str),
    /// A reference to an object of a type, or that object written inline.
    Source(&'static str),
    /// A reference, or a named object of the type written inline.
    NamedSource(&'static str),
}

/// A record's fields, or a variant's, in order.
pub type Fields = Vec<(&'static str, Shape)>;

/// A named type's definition.
#[derive(Clone, Debug, PartialEq)]
pub enum Definition {
    Record(Fields),
    /// Each variant with its fields, or `None` for a bare variant.
    Variants(Vec<(&'static str, Option<Fields>)>),
}

/// Every named type reachable from the document types.
#[derive(Default, Debug)]
pub struct Schema {
    pub types: IndexMap<&'static str, Option<Definition>>,
}

impl Schema {
    /// Register `name` once, defining it with `define` unless it is already
    /// registered or being defined.
    pub fn named(
        &mut self,
        name: &'static str,
        define: impl FnOnce(&mut Self) -> Definition,
    ) -> Shape {
        if !self.types.contains_key(name) {
            self.types.insert(name, None);
            let definition = define(self);
            self.types.insert(name, Some(definition));
        }
        Shape::Named(name)
    }
}

macro_rules! integer {
    ($($ty:ty),*) => {$(
        impl Data for $ty {
            fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
                match value.value {
                    DataValue::Integer(number) => match <$ty>::try_from(number) {
                        Ok(number) => Some(number),
                        Err(_) => {
                            decoder.error(
                                value.span,
                                format!(
                                    "expected an integer from {} to {}",
                                    <$ty>::MIN,
                                    <$ty>::MAX
                                ),
                            );
                            None
                        }
                    },
                    _ => decoder.mismatch(value, "an integer"),
                }
            }
            fn encode(&self) -> DataValue {
                DataValue::Integer(i64::from(*self))
            }
            fn shape(_: &mut Schema) -> Shape {
                Shape::Integer
            }
        }
    )*};
}

integer!(u8, u16, u32, i32, i64);

macro_rules! scalar {
    ($ty:ty, $variant:ident, $shape:ident, $expected:literal) => {
        impl Data for $ty {
            fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
                match &value.value {
                    DataValue::$variant(inner) => Some(inner.clone()),
                    _ => decoder.mismatch(value, $expected),
                }
            }
            fn encode(&self) -> DataValue {
                DataValue::$variant(self.clone())
            }
            fn shape(_: &mut Schema) -> Shape {
                Shape::$shape
            }
        }
    };
}

impl Data for f32 {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match &value.value {
            DataValue::Float(inner) => Some(*inner),
            // Data never widens an integer, which would change its spelling.
            DataValue::Integer(integer) => {
                decoder.diagnostics.push(
                    Diagnostic::new(value.span, format!("expected a float: write `{integer}.0`"))
                        .with_fix(format!("{integer}.0")),
                );
                None
            }
            _ => decoder.mismatch(value, "a float like `1.0`"),
        }
    }
    fn encode(&self) -> DataValue {
        DataValue::Float(*self)
    }
    fn shape(_: &mut Schema) -> Shape {
        Shape::Float
    }
}

scalar!(bool, Bool, Bool, "`true` or `false`");
scalar!(String, String, String, "a string");
scalar!(Color, Color, Color, "a color like `#ff8800`");
scalar!(Duration, Duration, Duration, "a duration like `1.5s`");

/// Meters, exact to the micrometer: `0.35m`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Meters(pub i64);

impl Data for Meters {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match value.value {
            DataValue::Distance(micrometers) => Some(Self(micrometers)),
            _ => decoder.mismatch(value, "a distance like `0.35m`"),
        }
    }
    fn encode(&self) -> DataValue {
        DataValue::Distance(self.0)
    }
    fn shape(_: &mut Schema) -> Shape {
        Shape::Distance
    }
}

/// A relative path: `<audio/song.mp3>`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Path(pub String);

impl Data for Path {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match &value.value {
            DataValue::Path(path) => Some(Self(path.clone())),
            _ => decoder.mismatch(value, "a path like `<audio/song.mp3>`"),
        }
    }
    fn encode(&self) -> DataValue {
        DataValue::Path(self.0.clone())
    }
    fn shape(_: &mut Schema) -> Shape {
        Shape::Path
    }
}

/// An object's own name, written bare: `name: output_01`.
#[derive(Clone, Debug)]
pub struct Name(pub Spanned<Identifier>);

impl PartialEq for Name {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Name {
    pub fn new(name: Identifier) -> Self {
        Self(Spanned::new(name, NO_SPAN))
    }

    pub fn as_str(&self) -> &str {
        self.0.value.as_str()
    }
}

impl Data for Name {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match &value.value {
            DataValue::Reference(segments) if segments.len() == 1 => {
                Some(Self(segments[0].clone()))
            }
            _ => decoder.mismatch(value, "a snake_case name"),
        }
    }
    fn encode(&self) -> DataValue {
        DataValue::Reference(vec![self.0.clone()])
    }
    fn shape(_: &mut Schema) -> Shape {
        Shape::Name
    }
}

/// A reference to another object, resolved after decoding:
/// `alias.name`, `layout.fixture`.
#[derive(Clone, Debug)]
pub struct Reference {
    pub segments: Vec<Spanned<Identifier>>,
    pub span: TextSpan,
}

impl PartialEq for Reference {
    fn eq(&self, other: &Self) -> bool {
        self.segments == other.segments
    }
}

impl Reference {
    pub fn new(segments: Vec<Identifier>) -> Self {
        Self {
            segments: segments
                .into_iter()
                .map(|segment| Spanned::new(segment, NO_SPAN))
                .collect(),
            span: NO_SPAN,
        }
    }

    pub fn text(&self) -> String {
        self.segments
            .iter()
            .map(|segment| segment.value.as_str())
            .collect::<Vec<_>>()
            .join(".")
    }
}

impl Data for Reference {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match &value.value {
            DataValue::Reference(segments) => Some(Self {
                segments: segments.clone(),
                span: value.span,
            }),
            _ => decoder.mismatch(value, "a reference"),
        }
    }
    fn encode(&self) -> DataValue {
        DataValue::Reference(self.segments.clone())
    }
    fn shape(_: &mut Schema) -> Shape {
        Shape::Reference
    }
}

/// A reference to an object, or the object written in place, which its
/// owner then owns: `definition: fixtures.strip` or `definition: Fixture { ... }`.
#[derive(Clone, Debug, PartialEq)]
pub enum Source<T> {
    Reference(Reference),
    Inline(T),
}

impl<T: Record> Data for Source<T> {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match &value.value {
            DataValue::Reference(_) => Reference::decode(value, decoder).map(Self::Reference),
            DataValue::Record(..) => decode_record(value, decoder).map(Self::Inline),
            _ => decoder.mismatch(value, &format!("a reference or `{} {{ ... }}`", T::TYPE)),
        }
    }
    fn encode(&self) -> DataValue {
        match self {
            Self::Reference(reference) => reference.encode(),
            Self::Inline(inline) => inline.encode(),
        }
    }
    fn shape(schema: &mut Schema) -> Shape {
        T::shape(schema);
        Shape::Source(T::TYPE)
    }
}

/// A reference to an object, or a named object written in place, which its
/// owner then owns: `controllers: [shared, Controller main { ... }]`.
#[derive(Clone, Debug, PartialEq)]
pub enum NamedSource<T> {
    Reference(Reference),
    Inline(Name, T),
}

impl<T: Record> Data for NamedSource<T> {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match &value.value {
            DataValue::Reference(_) => Reference::decode(value, decoder).map(Self::Reference),
            DataValue::Named(ty, name, fields) if ty.value.as_str() == T::TYPE => {
                T::decode_fields(fields, decoder)
                    .map(|inline| Self::Inline(Name(name.clone()), inline))
            }
            _ => decoder.mismatch(
                value,
                &format!("a reference or `{} name {{ ... }}`", T::TYPE),
            ),
        }
    }
    fn encode(&self) -> DataValue {
        match self {
            Self::Reference(reference) => reference.encode(),
            Self::Inline(name, inline) => DataValue::Named(
                Spanned::new(identifier(T::TYPE), NO_SPAN),
                name.0.clone(),
                Spanned::new(inline.encode_fields(), NO_SPAN),
            ),
        }
    }
    fn shape(schema: &mut Schema) -> Shape {
        T::shape(schema);
        Shape::NamedSource(T::TYPE)
    }
}

/// Parameter values keyed by parameter name, typed later by their definition.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Params(pub Vec<(Spanned<Identifier>, Spanned<DataValue>)>);

impl Data for Params {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match &value.value {
            DataValue::Map(fields) => Some(Self(
                fields
                    .iter()
                    .map(|field| (field.name.clone(), field.value.clone()))
                    .collect(),
            )),
            _ => decoder.mismatch(value, "parameter values like `{ speed: 1.0 }`"),
        }
    }
    fn encode(&self) -> DataValue {
        DataValue::Map(
            self.0
                .iter()
                .map(|(name, value)| DataField {
                    name: name.clone(),
                    value: value.clone(),
                })
                .collect(),
        )
    }
    fn shape(_: &mut Schema) -> Shape {
        Shape::Params
    }
}

impl<T: Data> Data for Option<T> {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match value.value {
            DataValue::None => Some(None),
            _ => T::decode(value, decoder).map(Some),
        }
    }
    fn encode(&self) -> DataValue {
        match self {
            None => DataValue::None,
            Some(value) => value.encode(),
        }
    }
    fn shape(schema: &mut Schema) -> Shape {
        Shape::Option(Box::new(T::shape(schema)))
    }
}

impl<T: Data> Data for Vec<T> {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match &value.value {
            DataValue::List(items) => {
                let decoded = items
                    .iter()
                    .map(|item| T::decode(item, decoder))
                    .collect::<Vec<_>>();
                decoded.into_iter().collect()
            }
            _ => decoder.mismatch(value, "a list"),
        }
    }
    fn encode(&self) -> DataValue {
        DataValue::List(self.iter().map(|item| spanned(item.encode())).collect())
    }
    fn shape(schema: &mut Schema) -> Shape {
        Shape::List(Box::new(T::shape(schema)))
    }
}

macro_rules! tuple {
    ($count:literal: $($name:ident $index:tt),*) => {
        impl<$($name: Data),*> Data for ($($name,)*) {
            fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
                match &value.value {
                    DataValue::Tuple(items) if items.len() == $count => {
                        let decoded = ($($name::decode(&items[$index], decoder),)*);
                        Some(($(decoded.$index?,)*))
                    }
                    _ => decoder.mismatch(value, concat!("a tuple of ", $count, " items")),
                }
            }
            fn encode(&self) -> DataValue {
                DataValue::Tuple(vec![$(spanned(self.$index.encode())),*])
            }
            fn shape(schema: &mut Schema) -> Shape {
                Shape::Tuple(vec![$($name::shape(schema)),*])
            }
        }
    };
}

tuple!(2: A 0, B 1);
tuple!(3: A 0, B 1, C 2);
tuple!(4: A 0, B 1, C 2, D 3);
