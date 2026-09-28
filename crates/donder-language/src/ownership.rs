//! Authored ownership is independent of the document containing a reusable value.
pub mod edit;
mod validation;
pub(crate) use validation::validate_ownership;

/// A value owned by its parent, or an explicit link to a reusable value.
///
/// Inline values are copied with and deleted with their owner. References retain
/// shared identity whether the target is in the same document or another one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueSource<T, I> {
    Inline(T),
    Reference(I),
}

impl<T, I> ValueSource<T, I> {
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

impl<T: Identified<Id = I>, I> ValueSource<T, I> {
    pub fn id(&self) -> &I {
        match self {
            Self::Inline(value) => value.id(),
            Self::Reference(id) => id,
        }
    }
}
