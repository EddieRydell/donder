//! Import a profile's layout, controllers and patch, from xLights or Vixen,
//! into a Donder project.
//!
//! Each source's reader turns its profile into the fixtures, controllers and
//! groups of [`setup`], whose pixels sit where the source draws them, in
//! channel order; one builder then makes them the project's setup.
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

mod fit;
mod report;
mod setup;
mod vixen;
mod xlights;
mod xml;

use camino::Utf8Path;
use donder_project_io::ProjectSession;

pub use report::{
    ControllerSkip, DropReason, DroppedMember, ElementPatch, FieldError, GroupSkip, ImportOptions,
    ImportReport, Scale, ShapeSkip, SkipReason, Skipped, SkippedController, SkippedGroup,
    TYPICAL_PIXEL_SPACING, Unrouted, UnroutedReason,
};
pub use setup::ImportError;

/// The program a profile folder comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// An xLights show folder holding `xlights_rgbeffects.xml`.
    XLights,
    /// A profile folder holding `SystemData/SystemConfig.xml`.
    Vixen,
}

impl Source {
    /// The program whose profile the folder holds.
    pub fn detect(folder: &Utf8Path) -> Result<Self, ImportError> {
        let xlights = folder.join(xlights::LAYOUT_FILE).is_file();
        let vixen = folder.join(vixen::SYSTEM_CONFIG_FILE).is_file();
        match (xlights, vixen) {
            (true, false) => Ok(Self::XLights),
            (false, true) => Ok(Self::Vixen),
            (true, true) => Err(ImportError::AmbiguousProfile(folder.to_owned())),
            (false, false) => Err(ImportError::UnknownProfile(folder.to_owned())),
        }
    }
}

/// Import the profile in `folder` into the project, whose setup must be empty.
pub fn import_profile(
    session: &mut ProjectSession,
    folder: &Utf8Path,
    options: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    if let Some(spacing) = options.pixel_spacing
        && !(spacing.is_finite() && spacing > 0.0)
    {
        return Err(ImportError::PixelSpacing(spacing));
    }
    let profile = match Source::detect(folder)? {
        Source::XLights => xlights::read(folder, options)?,
        Source::Vixen => vixen::read(folder, options)?,
    };
    setup::build(session, profile)
}
