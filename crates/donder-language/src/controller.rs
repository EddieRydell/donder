use std::net::{IpAddr, SocketAddr};

use crate::identity::ObjectIdentity;

pub const DMX_SLOT_LIMIT: u16 = 512;
pub const E131_UNIVERSE_MIN: u16 = 1;
pub const E131_UNIVERSE_MAX: u16 = 63_999;
pub const ARTNET_PORT_ADDRESS_MAX: u16 = 32_767;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ControllerId(pub ObjectIdentity);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ControllerPortId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Controller {
    pub id: ControllerId,
    pub description: Option<String>,
    pub protocol: ControllerProtocol,
    pub ports: Vec<ControllerPort>,
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
    pub source_name: String,
    pub bind_address: IpAddr,
    pub priority: u8,
    pub mode: E131Mode,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtNetMode {
    Unicast,
    Broadcast,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerPort {
    pub id: ControllerPortId,
    /// Unique within the controller; routes use it.
    pub name: crate::dsl::Identifier,
    pub address: ControllerPortAddress,
    pub slot_count: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ControllerPortAddress {
    E131Universe(u16),
    ArtNetPort(u16),
    /// A Donder controller's physical output, numbered from 1.
    DonderOutput(u8),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerValidationError {
    EmptySourceName,
    InvalidPriority(u8),
    DuplicatePort(ControllerPortId),
    /// Port names are `snake_case` and unique within their controller.
    InvalidPortName(ControllerPortId),
    EmptyPort(ControllerPortId),
    TooManySlots {
        port: ControllerPortId,
        slots: u16,
    },
    ProtocolAddressMismatch(ControllerPortId),
    InvalidE131Universe {
        port: ControllerPortId,
        universe: u16,
    },
    InvalidArtNetPort {
        port: ControllerPortId,
        address: u16,
    },
    InvalidDonderOutput {
        port: ControllerPortId,
        output: u8,
    },
    DuplicateProtocolAddress(ControllerPortAddress),
}

impl Controller {
    pub fn validate(&self) -> Result<(), ControllerValidationError> {
        if let ControllerProtocol::E131(config) = &self.protocol {
            if config.source_name.trim().is_empty() {
                return Err(ControllerValidationError::EmptySourceName);
            }
            if !(1..=200).contains(&config.priority) {
                return Err(ControllerValidationError::InvalidPriority(config.priority));
            }
        }
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        let mut addresses = std::collections::HashSet::new();
        for port in &self.ports {
            if !ids.insert(port.id) {
                return Err(ControllerValidationError::DuplicatePort(port.id));
            }
            if !crate::names::is_object_name(port.name.as_str()) || !names.insert(&port.name) {
                return Err(ControllerValidationError::InvalidPortName(port.id));
            }
            if port.slot_count == 0 {
                return Err(ControllerValidationError::EmptyPort(port.id));
            }
            // A Donder controller reports its own output widths when it admits a show.
            if !matches!(self.protocol, ControllerProtocol::Donder(_))
                && port.slot_count > DMX_SLOT_LIMIT
            {
                return Err(ControllerValidationError::TooManySlots {
                    port: port.id,
                    slots: port.slot_count,
                });
            }
            if !addresses.insert(port.address) {
                return Err(ControllerValidationError::DuplicateProtocolAddress(
                    port.address,
                ));
            }
            match (&self.protocol, port.address) {
                (ControllerProtocol::E131(_), ControllerPortAddress::E131Universe(universe)) => {
                    if !(E131_UNIVERSE_MIN..=E131_UNIVERSE_MAX).contains(&universe) {
                        return Err(ControllerValidationError::InvalidE131Universe {
                            port: port.id,
                            universe,
                        });
                    }
                }
                (ControllerProtocol::ArtNet(_), ControllerPortAddress::ArtNetPort(address)) => {
                    if address > ARTNET_PORT_ADDRESS_MAX {
                        return Err(ControllerValidationError::InvalidArtNetPort {
                            port: port.id,
                            address,
                        });
                    }
                }
                // Unique outputs within 1..=ports map one-to-one onto device lanes.
                (ControllerProtocol::Donder(_), ControllerPortAddress::DonderOutput(output)) => {
                    if output == 0 || usize::from(output) > self.ports.len() {
                        return Err(ControllerValidationError::InvalidDonderOutput {
                            port: port.id,
                            output,
                        });
                    }
                }
                _ => return Err(ControllerValidationError::ProtocolAddressMismatch(port.id)),
            }
        }
        Ok(())
    }
}

impl crate::ownership::Identified for Controller {
    type Id = ControllerId;
    fn id(&self) -> &Self::Id {
        &self.id
    }
}

pub type ControllerSource = crate::ownership::ValueSource<Box<Controller>, ControllerId>;

impl AsRef<ObjectIdentity> for ControllerId {
    fn as_ref(&self) -> &ObjectIdentity {
        &self.0
    }
}
