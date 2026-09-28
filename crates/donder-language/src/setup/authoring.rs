//! Typed setup authoring. Callers own transactionality and source registration.

mod controllers;
pub use controllers::{attach_controller, detach_controller};
