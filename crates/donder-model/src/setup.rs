pub(crate) use crate::identity::SetupId;

pub(crate) mod authoring;

#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub id: SetupId,
    pub description: Option<String>,
    pub layout: crate::layout::OwnedLayout,
    pub patch: crate::patch::OwnedPatch,
    pub controllers: Vec<crate::controller::OwnedController>,
    pub input_devices: Vec<crate::input_device::OwnedInputDevice>,
}

impl crate::ownership::Identified for Setup {
    type Id = SetupId;
    fn id(&self) -> &Self::Id {
        &self.id
    }
}

pub type OwnedSetup = crate::ownership::Owned<Box<Setup>, SetupId>;
