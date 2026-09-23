use donder_language::controller::{ControllerId, ControllerPortId};
use donder_runtime::signal::RenderedFixture;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerPortFrame {
    pub controller: ControllerId,
    pub port: ControllerPortId,
    pub slots: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderedSequenceFrame {
    pub frame_index: u32,
    pub frame_rate: u32,
    pub sample_time: donder_language::values::SampleTime,
    pub fixtures: Vec<RenderedFixture>,
    pub controller_frames: Vec<ControllerPortFrame>,
}

impl AsMut<[u8]> for ControllerPortFrame {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.slots
    }
}
