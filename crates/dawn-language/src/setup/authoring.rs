//! Typed setup authoring. Callers own transactionality and source registration.

mod controllers;
mod layout;
pub use controllers::{attach_controller, copy_controller, detach_controller};
pub use layout::copy_setup_layout;
