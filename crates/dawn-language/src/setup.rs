use crate::controller::ControllerId;
use crate::element::ElementTreeId;
use crate::identity::SourceIdentity;
use crate::patch::PatchId;
use crate::preview::PreviewLayoutId;

pub mod authoring;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SetupId(pub SourceIdentity);

#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub id: SetupId,
    pub elements: ElementTreeId,
    pub preview: PreviewLayoutId,
    pub patch: PatchId,
    pub controllers: Vec<ControllerId>,
}
