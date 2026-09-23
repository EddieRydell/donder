use crate::controller::ControllerId;
use crate::identity::SourceIdentity;
use crate::layout::LayoutId;
use crate::patch::PatchId;

pub mod authoring;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SetupId(pub SourceIdentity);

#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub id: SetupId,
    pub layout: LayoutId,
    pub patch: PatchId,
    pub controllers: Vec<ControllerId>,
}
