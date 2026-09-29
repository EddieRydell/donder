#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

pub mod automation;
pub mod bindings;
pub mod dsl;
mod evaluation;
pub mod patch;
pub mod sampling;
pub mod sequence;
pub mod signal;
pub mod values;
pub mod wire;
