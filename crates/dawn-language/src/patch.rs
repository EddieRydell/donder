//! LED output routing. Pixel ranges describe physical wiring, not effect targets.

use crate::controller::{ControllerId, ControllerPortId};
use crate::identity::SourceIdentity;
use crate::layout::FixtureTarget;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PatchId(pub SourceIdentity);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PixelRouteId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub id: PatchId,
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
    pub port: ControllerPortId,
    /// Zero-based byte offset in the controller port.
    pub start_slot: u16,
    pub encoding: PixelEncoding,
    pub gamma: f32,
    pub brightness: f32,
}

pub use dawn_runtime::patch::PixelEncoding;

impl Patch {
    pub fn remove_output(&mut self, id: PixelRouteId) -> Result<(), String> {
        let index = self
            .routes
            .iter()
            .position(|route| route.id == id)
            .ok_or("Output assignment was not found.")?;
        self.routes.remove(index);
        Ok(())
    }
}
