//! Import an xLights show's layout, controllers and patch into a Donder
//! project.
//!
//! xLights keeps its layout in `xlights_rgbeffects.xml` and its controllers
//! in `xlights_networks.xml`. Each model becomes a fixture whose pixels sit
//! where xLights draws its nodes, in channel order, and its channels become
//! routes on the controller ports that carry them.
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

mod channels;
mod fit;
mod kind;
mod models;
mod networks;
mod placement;
mod project;
mod read;
mod report;
mod xml;

pub use project::{ImportError, LAYOUT_FILE, NETWORKS_FILE, import_show};
pub use report::{
    ControllerSkip, DropReason, DroppedMember, ImportOptions, ImportReport, Scale, SkipReason,
    Skipped, SkippedController, TYPICAL_PIXEL_SPACING, Unrouted, UnroutedReason,
};
