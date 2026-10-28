pub(crate) use crate::identity::SetupId;

pub(crate) mod authoring;

#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub id: SetupId,
    pub description: Option<String>,
    pub layout: crate::layout::LayoutSource,
    pub patch: crate::patch::PatchSource,
    pub controllers: Vec<crate::controller::ControllerSource>,
    pub input_devices: Vec<crate::input_device::InputDeviceSource>,
}

impl crate::ownership::Identified for Setup {
    type Id = SetupId;
    fn id(&self) -> &Self::Id {
        &self.id
    }
}

pub type SetupSource = crate::ownership::ValueSource<Box<Setup>, SetupId>;
