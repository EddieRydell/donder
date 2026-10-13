//! Checked typed setup authoring. Callers own source registration.

mod controllers;
mod input_devices;
pub use controllers::{attach_controller, detach_controller};
pub use input_devices::{attach_input_device, detach_input_device};
