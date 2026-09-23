use std::net::{IpAddr, SocketAddr};

use crate::identity::SourceIdentity;

pub const DMX_SLOT_LIMIT: u16 = 512;
pub const E131_UNIVERSE_MIN: u16 = 1;
pub const E131_UNIVERSE_MAX: u16 = 63_999;
pub const ARTNET_PORT_ADDRESS_MAX: u16 = 32_767;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ControllerId(pub SourceIdentity);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ControllerPortId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Controller {
    pub protocol: ControllerProtocol,
    pub ports: Vec<ControllerPort>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerProtocol {
    E131(E131Config),
    ArtNet(ArtNetConfig),
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
    pub address: ControllerPortAddress,
    pub slot_count: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ControllerPortAddress {
    E131Universe(u16),
    ArtNetPort(u16),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerValidationError {
    EmptySourceName,
    InvalidPriority(u8),
    DuplicatePort(ControllerPortId),
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
        let mut addresses = std::collections::HashSet::new();
        for port in &self.ports {
            if !ids.insert(port.id) {
                return Err(ControllerValidationError::DuplicatePort(port.id));
            }
            if port.slot_count == 0 {
                return Err(ControllerValidationError::EmptyPort(port.id));
            }
            if port.slot_count > DMX_SLOT_LIMIT {
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
                _ => return Err(ControllerValidationError::ProtocolAddressMismatch(port.id)),
            }
        }
        Ok(())
    }
}
