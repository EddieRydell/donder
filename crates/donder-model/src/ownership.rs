//! Authored ownership is independent of the document containing a reusable value.
pub(crate) mod edit;
mod validation;
pub(crate) use validation::validate_ownership;

use donder_runtime_types::Identifier;

/// A value owned by its parent, or an explicit link to a reusable value.
///
/// Inline values are copied with and deleted with their owner. References retain
/// shared identity whether the target is in the same document or another one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owned<T, I> {
    Inline(T),
    Reference(I),
}

impl<T, I> Owned<T, I> {
    pub fn inline(&self) -> Option<&T> {
        match self {
            Self::Inline(value) => Some(value),
            Self::Reference(_) => None,
        }
    }

    pub fn inline_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Inline(value) => Some(value),
            Self::Reference(_) => None,
        }
    }
}

/// A member of an owner's collection: one owned in place under a name unique
/// among the owner's members, or a link to a reusable value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedMember<T, I> {
    Inline { name: Identifier, value: Box<T> },
    Reference(I),
}

impl<T, I> OwnedMember<T, I> {
    pub fn inline(&self) -> Option<&T> {
        match self {
            Self::Inline { value, .. } => Some(value),
            Self::Reference(_) => None,
        }
    }

    pub fn inline_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Inline { value, .. } => Some(value),
            Self::Reference(_) => None,
        }
    }

    /// The name an owned member is stored under.
    pub fn name(&self) -> Option<&Identifier> {
        match self {
            Self::Inline { name, .. } => Some(name),
            Self::Reference(_) => None,
        }
    }
}

pub trait Identified {
    type Id;
    fn id(&self) -> &Self::Id;
}

impl<T: Identified> Identified for Box<T> {
    type Id = T::Id;
    fn id(&self) -> &Self::Id {
        self.as_ref().id()
    }
}

impl<T: Identified<Id = I>, I> Owned<T, I> {
    pub fn id(&self) -> &I {
        match self {
            Self::Inline(value) => value.id(),
            Self::Reference(id) => id,
        }
    }
}

impl<T: Identified<Id = I>, I> OwnedMember<T, I> {
    pub fn id(&self) -> &I {
        match self {
            Self::Inline { value, .. } => value.id(),
            Self::Reference(id) => id,
        }
    }
}
