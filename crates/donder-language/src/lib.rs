#![cfg_attr(not(feature = "host"), no_std)]
#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

extern crate alloc;

// Shared executable values use one pointer policy across compiler and runtime.
#[cfg(not(feature = "atomic"))]
pub use alloc::rc::Rc as Shared;
#[cfg(feature = "atomic")]
pub use alloc::sync::Arc as Shared;

pub mod automation;
#[cfg(feature = "host")]
pub mod controller;
pub mod dsl;
#[cfg(feature = "host")]
pub mod effect;
pub mod execution;
#[cfg(feature = "host")]
pub mod fixture;
#[cfg(feature = "host")]
pub mod geometry;
#[cfg(feature = "host")]
pub mod identity;
#[cfg(feature = "host")]
pub mod imports;
#[cfg(feature = "host")]
pub use imports::{
    ImportAlias, ImportDeclaration, ImportSource, SourceReference, is_valid_import_alias,
};
#[cfg(feature = "host")]
pub mod layout;
#[cfg(feature = "host")]
pub mod model;
#[cfg(feature = "host")]
pub mod operator;
#[cfg(feature = "host")]
pub mod ownership;
#[cfg(feature = "host")]
pub mod patch;
pub mod sampling;
#[cfg(feature = "host")]
pub mod sequence;
#[cfg(feature = "host")]
pub mod setup;
#[cfg(feature = "host")]
pub mod source_remap;
#[cfg(feature = "host")]
pub mod validation;
pub mod values;
