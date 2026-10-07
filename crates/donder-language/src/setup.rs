use crate::identity::ObjectIdentity;

pub mod authoring;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SetupId(pub ObjectIdentity);

#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub id: SetupId,
    pub description: Option<String>,
    pub layout: crate::layout::LayoutSource,
    pub patch: crate::patch::PatchSource,
    pub controllers: Vec<crate::controller::ControllerSource>,
}

impl crate::ownership::Identified for Setup {
    type Id = SetupId;
    fn id(&self) -> &Self::Id {
        &self.id
    }
}

pub type SetupSource = crate::ownership::ValueSource<Box<Setup>, SetupId>;

impl AsRef<ObjectIdentity> for SetupId {
    fn as_ref(&self) -> &ObjectIdentity {
        &self.0
    }
}
