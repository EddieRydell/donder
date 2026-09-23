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

mod artnet;
mod e131;

use std::collections::HashMap;

use donder_elaboration::ControllerPortFrame;
use donder_language::controller::{Controller, ControllerId, ControllerProtocol};

pub use artnet::ArtNetSender;
pub use e131::E131Sender;

#[derive(Debug)]
pub enum OutputError {
    MissingController(ControllerId),
    MissingPort {
        controller: ControllerId,
        port: donder_language::controller::ControllerPortId,
    },
    InvalidFrameLength {
        controller: ControllerId,
        expected: usize,
        actual: usize,
    },
    Socket {
        controller: ControllerId,
        message: String,
    },
    Codec {
        controller: ControllerId,
        message: String,
    },
}

pub enum ControllerSender {
    E131(E131Sender),
    ArtNet(ArtNetSender),
}

impl ControllerSender {
    pub fn open(id: ControllerId, controller: &Controller) -> Result<Self, OutputError> {
        match &controller.protocol {
            ControllerProtocol::E131(config) => {
                E131Sender::open(id, config, &controller.ports).map(Self::E131)
            }
            ControllerProtocol::ArtNet(config) => {
                ArtNetSender::open(id, config, &controller.ports).map(Self::ArtNet)
            }
        }
    }

    pub fn send<'a>(
        &mut self,
        frames: impl IntoIterator<Item = &'a ControllerPortFrame>,
    ) -> Result<(), OutputError> {
        match self {
            Self::E131(sender) => sender.send(frames),
            Self::ArtNet(sender) => sender.send(frames),
        }
    }

    pub fn blackout(&mut self) -> Result<(), OutputError> {
        match self {
            Self::E131(sender) => sender.blackout(),
            Self::ArtNet(sender) => sender.blackout(),
        }
    }

    pub fn terminate(&mut self) -> Result<(), OutputError> {
        match self {
            Self::E131(sender) => sender.terminate(),
            Self::ArtNet(sender) => sender.blackout(),
        }
    }
}

pub struct OutputTransports {
    senders: HashMap<ControllerId, ControllerSender>,
}

impl OutputTransports {
    pub fn open(
        controllers: &indexmap::IndexMap<ControllerId, Controller>,
        active: &[ControllerId],
    ) -> Result<Self, OutputError> {
        let mut senders = HashMap::new();
        for id in active {
            let definition = controllers
                .get(id)
                .ok_or_else(|| OutputError::MissingController(id.clone()))?;
            senders.insert(id.clone(), ControllerSender::open(id.clone(), definition)?);
        }
        Ok(Self { senders })
    }

    pub fn send(&mut self, frames: &[ControllerPortFrame]) -> Result<(), OutputError> {
        for (id, sender) in &mut self.senders {
            sender.send(frames.iter().filter(|frame| &frame.controller == id))?;
        }
        Ok(())
    }

    pub fn blackout_and_terminate(&mut self) -> Result<(), OutputError> {
        let mut first_error = None;
        for sender in self.senders.values_mut() {
            let blackout = sender.blackout();
            let termination = sender.terminate();
            if let Err(error) = blackout.and(termination)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}
