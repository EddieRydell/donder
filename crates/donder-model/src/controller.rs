use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::ops::Range;

pub(crate) use crate::identity::ControllerId;

pub(crate) const DMX_SLOT_LIMIT: u16 = 512;
pub(crate) const E131_UNIVERSE_MIN: u16 = 1;
pub(crate) const E131_UNIVERSE_MAX: u16 = 63_999;
pub(crate) const ARTNET_PORT_ADDRESS_MAX: u16 = 32_767;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ControllerPortId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Controller {
    pub description: Option<String>,
    pub protocol: ControllerProtocol,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerProtocol {
    E131(E131Config),
    ArtNet(ArtNetConfig),
    /// A Donder controller plays prepared shows itself; the editor uploads
    /// them and schedules playback instead of streaming frames.
    Donder(DonderConfig),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DonderConfig {
    pub device: DonderDeviceId,
    /// Its physical outputs, each its own channel stream.
    pub ports: Vec<DonderPort>,
}

/// A Donder controller's port and the physical output it drives.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DonderPort {
    pub port: ControllerPort,
    /// The controller's physical output, numbered from 1.
    pub output: u8,
    pub slot_count: u16,
}

/// A Donder controller's factory MAC address as 12 lowercase hex digits.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct DonderDeviceId(String);

impl DonderDeviceId {
    pub fn parse(value: &str) -> Option<Self> {
        (value.len() == 12
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        .then(|| Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct E131Config {
    pub sender_name: String,
    pub bind_address: IpAddr,
    pub priority: u8,
    pub mode: E131Mode,
    pub universes: Universes,
    /// Where FSEQ files place its first channel, from 1 as FPP's channel
    /// outputs number them; none leaves it out of FSEQ files.
    pub fseq_channel: Option<NonZeroU32>,
    /// Its physical outputs, in the order its channels fill them.
    pub ports: Vec<ControllerPort>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum E131Mode {
    Multicast,
    Unicast { destination: IpAddr },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtNetConfig {
    pub bind_address: SocketAddr,
    pub destination: SocketAddr,
    pub mode: ArtNetMode,
    pub universes: Universes,
    /// Where FSEQ files place its first channel, from 1 as FPP's channel
    /// outputs number them; none leaves it out of FSEQ files.
    pub fseq_channel: Option<NonZeroU32>,
    /// Its physical outputs, in the order its channels fill them.
    pub ports: Vec<ControllerPort>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtNetMode {
    Unicast,
    Broadcast,
}

/// The consecutive universes an E1.31 or Art-Net controller sends, each of
/// the same width. Its channels are numbered across them, so a route's
/// channels, even one pixel's, run on from one universe into the next.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Universes {
    /// The first universe: an E1.31 universe number, or an Art-Net 15-bit
    /// port-address.
    pub start: u16,
    pub count: u16,
    /// Channels each universe carries, at most 512.
    pub channels: u16,
}

impl Universes {
    /// The controller's channels across all its universes.
    pub fn channel_count(&self) -> u32 {
        u32::from(self.count) * u32::from(self.channels)
    }

    /// Each universe, with its zero-based range of the controller's channels.
    pub fn iter(&self) -> impl Iterator<Item = (u16, Range<u32>)> + use<> {
        let Self {
            start,
            count,
            channels,
        } = *self;
        let width = u32::from(channels);
        (0..count).map(move |index| {
            let first = u32::from(index) * width;
            (start + index, first..first + width)
        })
    }

    /// The universe and channel from 1 of a zero-based controller channel.
    pub fn locate(&self, channel: u32) -> UniverseChannel {
        let width = u32::from(self.channels);
        UniverseChannel {
            universe: self.start + (channel / width) as u16,
            channel: (channel % width) as u16 + 1,
        }
    }
}

/// A universe and one of its channels, from 1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UniverseChannel {
    pub universe: u16,
    pub channel: u16,
}

/// One byte stream a controller plays: an E1.31 or Art-Net controller's
/// channels, which it sends across its universes, or one Donder output.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum ControllerOutput {
    Universes,
    Port(ControllerPortId),
}

impl ControllerOutput {
    /// The port number a prepared show's output record carries: the Donder
    /// port's id, or 0 for a controller's universes.
    pub fn runtime_port(self) -> u32 {
        match self {
            Self::Universes => 0,
            Self::Port(port) => port.0,
        }
    }
}

/// Where a nonempty range of an output's channels lands, as people count:
/// universes and channels from 1, or a Donder output's channels from 1.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChannelPlace {
    Universes {
        first: UniverseChannel,
        last: UniverseChannel,
    },
    Port {
        port: String,
        first: u32,
        last: u32,
    },
}

/// One of a controller's physical outputs, as the controller names it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerPort {
    pub id: ControllerPortId,
    /// Free text, unique within the controller; routes use it.
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerValidationError {
    EmptySenderName,
    InvalidPriority(u8),
    NoUniverses,
    InvalidUniverseChannels(u16),
    E131UniversesOutOfRange(Universes),
    ArtNetUniversesOutOfRange(Universes),
    DuplicatePort(ControllerPortId),
    EmptyPortName(ControllerPortId),
    /// Port names are unique within their controller.
    DuplicatePortName(ControllerPortId),
    EmptyPort(ControllerPortId),
    InvalidDonderOutput {
        port: ControllerPortId,
        output: u8,
    },
    DuplicateDonderOutput(u8),
}

impl Controller {
    /// An E1.31 or Art-Net controller's universes.
    pub fn universes(&self) -> Option<&Universes> {
        match &self.protocol {
            ControllerProtocol::E131(config) => Some(&config.universes),
            ControllerProtocol::ArtNet(config) => Some(&config.universes),
            ControllerProtocol::Donder(_) => None,
        }
    }

    /// Where FSEQ files place an E1.31 or Art-Net controller's first
    /// channel, from 1.
    pub fn fseq_channel(&self) -> Option<NonZeroU32> {
        match &self.protocol {
            ControllerProtocol::E131(config) => config.fseq_channel,
            ControllerProtocol::ArtNet(config) => config.fseq_channel,
            ControllerProtocol::Donder(_) => None,
        }
    }

    /// Its physical outputs, in order.
    pub fn ports(&self) -> Vec<&ControllerPort> {
        match &self.protocol {
            ControllerProtocol::E131(config) => config.ports.iter().collect(),
            ControllerProtocol::ArtNet(config) => config.ports.iter().collect(),
            ControllerProtocol::Donder(config) => {
                config.ports.iter().map(|port| &port.port).collect()
            }
        }
    }

    pub fn port(&self, id: ControllerPortId) -> Option<&ControllerPort> {
        self.ports().into_iter().find(|port| port.id == id)
    }

    /// The channel stream a port plays in: an E1.31 or Art-Net controller's
    /// universes, which all its ports share, or a Donder port's own.
    /// `None` when the controller has no port `id`.
    pub fn port_output(&self, id: ControllerPortId) -> Option<ControllerOutput> {
        self.port(id)?;
        Some(self.own_port_output(id))
    }

    /// The channel stream one of the controller's own ports plays in.
    pub(crate) fn own_port_output(&self, id: ControllerPortId) -> ControllerOutput {
        match self.protocol {
            ControllerProtocol::E131(_) | ControllerProtocol::ArtNet(_) => {
                ControllerOutput::Universes
            }
            ControllerProtocol::Donder(_) => ControllerOutput::Port(id),
        }
    }

    /// A Donder controller's ports with their physical outputs; other
    /// controllers have none.
    pub fn donder_ports(&self) -> &[DonderPort] {
        match &self.protocol {
            ControllerProtocol::Donder(config) => &config.ports,
            ControllerProtocol::E131(_) | ControllerProtocol::ArtNet(_) => &[],
        }
    }

    /// The byte streams the controller plays, in order, with their widths.
    pub fn outputs(&self) -> Vec<(ControllerOutput, u32)> {
        match self.universes() {
            Some(universes) => vec![(ControllerOutput::Universes, universes.channel_count())],
            None => self
                .donder_ports()
                .iter()
                .map(|port| {
                    (
                        ControllerOutput::Port(port.port.id),
                        u32::from(port.slot_count),
                    )
                })
                .collect(),
        }
    }

    /// The width of one of the controller's outputs.
    pub fn output_width(&self, output: ControllerOutput) -> Option<u32> {
        match (self.universes(), output) {
            (Some(universes), ControllerOutput::Universes) => Some(universes.channel_count()),
            (None, ControllerOutput::Port(id)) => self
                .donder_ports()
                .iter()
                .find(|port| port.port.id == id)
                .map(|port| u32::from(port.slot_count)),
            _ => None,
        }
    }

    /// The output a prepared show's output record names by its port number.
    pub fn runtime_output(&self, port: u32) -> ControllerOutput {
        self.own_port_output(ControllerPortId(port))
    }

    /// One of the controller's own Donder ports, by an output of it.
    fn own_port(&self, id: ControllerPortId) -> &ControllerPort {
        let Some(port) = self.port(id) else {
            unreachable!("the controller's own outputs name its ports")
        };
        port
    }

    /// How people name one of the controller's own outputs: its universes,
    /// such as `universes 1–30`, or a Donder port's name.
    pub fn output_label(&self, output: ControllerOutput) -> String {
        match (self.universes(), output) {
            (Some(universes), ControllerOutput::Universes) if universes.count == 1 => {
                format!("universe {}", universes.start)
            }
            (Some(universes), ControllerOutput::Universes) => format!(
                "universes {}–{}",
                universes.start,
                u32::from(universes.start) + u32::from(universes.count) - 1
            ),
            (None, ControllerOutput::Port(id)) => self.own_port(id).name.clone(),
            _ => unreachable!("a controller's own outputs follow its protocol"),
        }
    }

    /// Where the nonempty zero-based `channels` of one of the controller's
    /// own outputs land; `None` for no channels.
    pub fn place(&self, output: ControllerOutput, channels: Range<u32>) -> Option<ChannelPlace> {
        if channels.is_empty() {
            return None;
        }
        Some(match (self.universes(), output) {
            (Some(universes), ControllerOutput::Universes) => ChannelPlace::Universes {
                first: universes.locate(channels.start),
                last: universes.locate(channels.end - 1),
            },
            (None, ControllerOutput::Port(id)) => ChannelPlace::Port {
                port: self.own_port(id).name.clone(),
                first: channels.start + 1,
                last: channels.end,
            },
            _ => unreachable!("a controller's own outputs follow its protocol"),
        })
    }

    pub fn validate(&self) -> Result<(), ControllerValidationError> {
        match &self.protocol {
            ControllerProtocol::E131(config) => {
                if config.sender_name.trim().is_empty() {
                    return Err(ControllerValidationError::EmptySenderName);
                }
                if !(1..=200).contains(&config.priority) {
                    return Err(ControllerValidationError::InvalidPriority(config.priority));
                }
                validate_universes(&config.universes)?;
                validate_ports(&config.ports)?;
                let universes = config.universes;
                if universes.start < E131_UNIVERSE_MIN
                    || u32::from(universes.start) + u32::from(universes.count) - 1
                        > u32::from(E131_UNIVERSE_MAX)
                {
                    return Err(ControllerValidationError::E131UniversesOutOfRange(
                        universes,
                    ));
                }
                Ok(())
            }
            ControllerProtocol::ArtNet(config) => {
                validate_universes(&config.universes)?;
                validate_ports(&config.ports)?;
                let universes = config.universes;
                if u32::from(universes.start) + u32::from(universes.count) - 1
                    > u32::from(ARTNET_PORT_ADDRESS_MAX)
                {
                    return Err(ControllerValidationError::ArtNetUniversesOutOfRange(
                        universes,
                    ));
                }
                Ok(())
            }
            ControllerProtocol::Donder(config) => {
                validate_ports(config.ports.iter().map(|port| &port.port))?;
                validate_donder_ports(&config.ports)
            }
        }
    }
}

fn validate_universes(universes: &Universes) -> Result<(), ControllerValidationError> {
    if universes.count == 0 {
        return Err(ControllerValidationError::NoUniverses);
    }
    if !(1..=DMX_SLOT_LIMIT).contains(&universes.channels) {
        return Err(ControllerValidationError::InvalidUniverseChannels(
            universes.channels,
        ));
    }
    Ok(())
}

fn validate_ports<'a>(
    ports: impl IntoIterator<Item = &'a ControllerPort>,
) -> Result<(), ControllerValidationError> {
    let mut ids = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    for port in ports {
        if !ids.insert(port.id) {
            return Err(ControllerValidationError::DuplicatePort(port.id));
        }
        if port.name.trim().is_empty() {
            return Err(ControllerValidationError::EmptyPortName(port.id));
        }
        if !names.insert(&port.name) {
            return Err(ControllerValidationError::DuplicatePortName(port.id));
        }
    }
    Ok(())
}

/// A Donder controller reports its own output widths when it admits a show.
fn validate_donder_ports(ports: &[DonderPort]) -> Result<(), ControllerValidationError> {
    let mut outputs = std::collections::HashSet::new();
    for DonderPort {
        port,
        output,
        slot_count,
    } in ports
    {
        if *slot_count == 0 {
            return Err(ControllerValidationError::EmptyPort(port.id));
        }
        // Unique outputs within 1..=ports map one-to-one onto device lanes.
        if *output == 0 || usize::from(*output) > ports.len() {
            return Err(ControllerValidationError::InvalidDonderOutput {
                port: port.id,
                output: *output,
            });
        }
        if !outputs.insert(*output) {
            return Err(ControllerValidationError::DuplicateDonderOutput(*output));
        }
    }
    Ok(())
}
