//! LED output routing. Pixel ranges describe physical wiring, not effect targets.
use crate::project::EditRejection;
use donder_runtime_types::PixelEncoding;

use crate::controller::{ControllerId, ControllerOutput};
pub(crate) use crate::identity::PatchId;
use crate::layout::FixtureTarget;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PixelRouteId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub id: PatchId,
    pub description: Option<String>,
    pub routes: Vec<PixelRoute>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelSpan {
    pub start: u32,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PixelRoute {
    pub id: PixelRouteId,
    pub target: FixtureTarget,
    /// None routes the complete target, including future pixel-count edits.
    pub pixels: Option<PixelSpan>,
    pub controller: ControllerId,
    /// The controller's universes, or one of a Donder controller's ports.
    pub output: ControllerOutput,
    /// The route's first channel in `output`, from 0: across all of an
    /// E1.31 or Art-Net controller's universes, or in a Donder port.
    pub start_channel: u32,
    pub encoding: PixelEncoding,
    pub gamma: f32,
    pub brightness: f32,
    /// Vary each byte over frames so it averages to the exact level, for
    /// outputs that receive every frame.
    pub dither: bool,
}

impl Patch {
    pub(crate) fn remove_output(&mut self, id: PixelRouteId) -> Result<(), EditRejection> {
        let index = self
            .routes
            .iter()
            .position(|route| route.id == id)
            .ok_or(EditRejection::MissingOutput)?;
        self.routes.remove(index);
        Ok(())
    }
}

impl crate::ownership::Identified for Patch {
    type Id = PatchId;
    fn id(&self) -> &Self::Id {
        &self.id
    }
}

pub type OwnedPatch = crate::ownership::Owned<Box<Patch>, PatchId>;
