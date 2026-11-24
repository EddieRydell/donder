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
mod capture_code;
mod e131;
mod fseq;
mod test_pattern;

use std::collections::HashMap;

use donder_model::{Controller, ControllerId, ControllerProtocol};

pub use artnet::ArtNetSender;
pub use capture_code::{CAPTURE_SLOT_SECONDS, CaptureCode, CaptureSlot};
pub use e131::E131Sender;
pub use fseq::{FseqError, FseqStep, encode_fseq, setup_fseq_channel};
pub use test_pattern::{OutputTest, TestError, TestPattern, TestSelection};

/// An E1.31 or Art-Net controller's channels across all its universes,
/// ready for network transport, which sends each universe's share.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputFrame {
    pub controller: ControllerId,
    pub slots: Vec<u8>,
}

impl AsMut<[u8]> for OutputFrame {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.slots
    }
}

#[derive(Debug)]
pub enum OutputError {
    MissingController(ControllerId),
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
    /// Donder controllers play uploaded shows; they are never streamed.
    NotStreamed(ControllerId),
}

impl std::fmt::Display for OutputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingController(_) => {
                formatter.write_str("a controller to stream to is missing from the project")
            }
            Self::InvalidFrameLength {
                expected, actual, ..
            } => write!(
                formatter,
                "a frame of {actual} channels was sent to a controller of {expected} channels"
            ),
            Self::Socket { message, .. } | Self::Codec { message, .. } => {
                formatter.write_str(message)
            }
            Self::NotStreamed(_) => {
                formatter.write_str("Donder controllers play uploaded shows and are not streamed")
            }
        }
    }
}

/// A controller's network failure: what it could not do, naming the address
/// when there is one, and why.
fn socket_error(
    controller: ControllerId,
    action: impl std::fmt::Display,
    error: impl std::fmt::Display,
) -> OutputError {
    OutputError::Socket {
        controller,
        message: format!("{action}: {error}"),
    }
}

/// A controller's packet that could not be encoded, and why.
fn codec_error(
    controller: ControllerId,
    action: impl std::fmt::Display,
    error: impl std::fmt::Display,
) -> OutputError {
    OutputError::Codec {
        controller,
        message: format!("{action}: {error}"),
    }
}

pub enum ControllerSender {
    E131(E131Sender),
    ArtNet(ArtNetSender),
}

impl ControllerSender {
    fn open(id: ControllerId, controller: &Controller) -> Result<Self, OutputError> {
        match &controller.protocol {
            ControllerProtocol::E131(config) => E131Sender::open(id, config).map(Self::E131),
            ControllerProtocol::ArtNet(config) => ArtNetSender::open(id, config).map(Self::ArtNet),
            ControllerProtocol::Donder(_) => Err(OutputError::NotStreamed(id)),
        }
    }

    fn send<'a>(
        &mut self,
        frames: impl IntoIterator<Item = &'a OutputFrame>,
    ) -> Result<(), OutputError> {
        match self {
            Self::E131(sender) => sender.send(frames),
            Self::ArtNet(sender) => sender.send(frames),
        }
    }

    fn blackout(&mut self) -> Result<(), OutputError> {
        match self {
            Self::E131(sender) => sender.blackout(),
            Self::ArtNet(sender) => sender.blackout(),
        }
    }

    fn terminate(&mut self) -> Result<(), OutputError> {
        match self {
            Self::E131(sender) => sender.terminate(),
            Self::ArtNet(_) => Ok(()),
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
                .ok_or(OutputError::MissingController(*id))?;
            senders.insert(*id, ControllerSender::open(*id, definition)?);
        }
        Ok(Self { senders })
    }

    pub fn send(&mut self, frames: &[OutputFrame]) -> Result<(), OutputError> {
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
