//! Typed setup authoring. Callers own transactionality and source registration.

mod control_output;
mod controllers;
mod direct_output;
mod fixture_output;
mod layout_copy;
pub(crate) mod routing;
pub use control_output::{
    ControlOutputAssignment, ControlOutputMapping, assign_control_output, replace_control_outputs,
    resize_control_outputs,
};
pub use controllers::{attach_controller, copy_controller, detach_controller};
pub use fixture_output::{assign_fixture_output, replace_fixture_outputs};
pub use layout_copy::{SetupLayoutCopy, copy_setup_layout};
pub use routing::{
    PixelOutputAssignment, assign_pixel_output, replace_pixel_outputs, update_color_capability,
};
