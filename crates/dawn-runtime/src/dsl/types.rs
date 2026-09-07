use crate::values::{Color, Curve, Gradient, Marks};
use alloc::boxed::Box;
#[cfg(not(feature = "atomic"))]
use alloc::rc::Rc as Arc;
use alloc::string::String;
#[cfg(feature = "atomic")]
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::borrow::Borrow;

#[derive(Clone, Debug, Eq, PartialEq, Hash, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Identifier(Arc<str>);

impl Identifier {
    pub fn new(value: String) -> Result<Self, IdentifierError> {
        if value.is_empty() {
            return Err(IdentifierError::Empty);
        }

        let mut chars = value.chars();
        let Some(first) = chars.next() else {
            return Err(IdentifierError::Empty);
        };

        if !is_identifier_start(first) {
            return Err(IdentifierError::InvalidStart);
        }

        if chars.any(|candidate| !is_identifier_continue(candidate)) {
            return Err(IdentifierError::InvalidCharacter);
        }

        Ok(Self(value.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for Identifier {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentifierError {
    Empty,
    InvalidStart,
    InvalidCharacter,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator, __S::Error: rkyv::rancor::Source))]
#[rkyv(deserialize_bounds(__D::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext)))]
pub enum Type {
    Void,
    Int,
    Float,
    Bool,
    Color,
    Signal,
    Marks,
    Timeline,
    Target,
    TargetItems,
    TargetItem,
    Curve,
    Gradient,
    Array(#[rkyv(omit_bounds)] Box<Type>),
    Enum(Vec<Identifier>),
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, __S::Error: rkyv::rancor::Source))]
#[rkyv(deserialize_bounds(__D: rkyv::de::Pooling, __D::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext)))]
pub enum Value {
    Void,
    Int(i32),
    Float(f32),
    Bool(bool),
    Color(Color),
    Marks(Arc<Marks>),
    Target(Arc<TargetValue>),
    TargetItems(Arc<TargetItemsValue>),
    TargetItem(Arc<TargetItemValue>),
    Curve(Arc<Curve>),
    Gradient(Arc<Gradient>),
    Array(#[rkyv(omit_bounds)] Arc<[Value]>),
    Enum(Identifier),
}

impl Type {
    pub fn accepts(&self, actual: &Self) -> bool {
        self == actual
            || matches!((self, actual), (Self::Float, Self::Int))
            || match (self, actual) {
                (Self::Enum(options), Self::Enum(values)) => {
                    values.iter().all(|value| options.contains(value))
                }
                (Self::Array(expected), Self::Array(actual)) => expected.accepts(actual),
                _ => false,
            }
    }

    pub fn accepts_value(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Void, Value::Void)
            | (Self::Int, Value::Int(_))
            | (Self::Float, Value::Float(_) | Value::Int(_))
            | (Self::Bool, Value::Bool(_))
            | (Self::Color, Value::Color(_))
            | (Self::Marks, Value::Marks(_))
            | (Self::Target, Value::Target(_))
            | (Self::TargetItems, Value::TargetItems(_))
            | (Self::TargetItem, Value::TargetItem(_))
            | (Self::Curve, Value::Curve(_))
            | (Self::Gradient, Value::Gradient(_)) => true,
            (Self::Enum(options), Value::Enum(value)) => options.contains(value),
            (Self::Array(ty), Value::Array(values)) => {
                values.iter().all(|value| ty.accepts_value(value))
            }
            _ => false,
        }
    }

    pub fn array(item_type: Self) -> Self {
        Self::Array(Box::new(item_type))
    }

    pub fn default_value(&self) -> Value {
        match self {
            Self::Void | Self::Signal | Self::Timeline => Value::Void,
            Self::Int => Value::Int(0),
            Self::Float => Value::Float(0.0),
            Self::Bool => Value::Bool(false),
            Self::Color => Value::Color(Color {
                red: 0,
                green: 0,
                blue: 0,
            }),
            Self::Marks => Value::Marks(Arc::new(Marks { marks: Vec::new() })),
            Self::Target => Value::Target(Arc::new(TargetValue { groups: Vec::new() })),
            Self::TargetItems => {
                Value::TargetItems(Arc::new(TargetItemsValue { groups: Vec::new() }))
            }
            Self::TargetItem => Value::TargetItem(Arc::new(TargetItemValue {
                pixels: Arc::from([]),
            })),
            Self::Curve => Value::Curve(Arc::new(Curve { points: Vec::new() })),
            Self::Gradient => Value::Gradient(Arc::new(Gradient { stops: Vec::new() })),
            Self::Array(_) => Value::Array(Arc::from([])),
            Self::Enum(options) => Value::Enum(options[0].clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct TargetValue {
    pub groups: Vec<Arc<TargetItemValue>>,
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct TargetItemsValue {
    pub groups: Vec<Arc<TargetItemValue>>,
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct TargetItemValue {
    pub pixels: Arc<[TargetPixelValue]>,
}

#[derive(Clone, Copy, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct TargetPixelValue {
    pub element_index: i32,
    pub element_cell_index: i32,
    pub pixel_index: i32,
    pub pixel_count: i32,
    pub pixel_fraction: f32,
}

fn is_identifier_start(candidate: char) -> bool {
    candidate == '_' || candidate.is_ascii_alphabetic()
}

fn is_identifier_continue(candidate: char) -> bool {
    candidate == '_' || candidate.is_ascii_alphanumeric()
}
