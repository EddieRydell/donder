use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};

use donder_elaboration::ControllerPortFrame;
use donder_language::controller::{
    ControllerId, ControllerPort, ControllerPortAddress, ControllerPortId, E131Config, E131Mode,
};
use sacn::packet::{
    ACN_SDT_MULTICAST_PORT, AcnRootLayerProtocol, DataPacketDmpLayer, DataPacketFramingLayer,
    E131RootLayer, E131RootLayerData,
};
use sacn::source::SacnSource;

use crate::OutputError;

pub struct E131Sender {
    id: ControllerId,
    source: SacnSource,
    termination_socket: UdpSocket,
    config: E131Config,
    universes: HashMap<ControllerPortId, (u16, usize)>,
    universe_data: HashMap<ControllerPortId, Vec<u8>>,
}

impl E131Sender {
    pub fn open(
        id: ControllerId,
        config: &E131Config,
        ports: &[ControllerPort],
    ) -> Result<Self, OutputError> {
        let bind = SocketAddr::new(config.bind_address, 0);
        let mut source = SacnSource::with_ip(&config.source_name, bind)
            .map_err(|error| socket_error(&id, error))?;
        let termination_socket = UdpSocket::bind(bind).map_err(|error| socket_error(&id, error))?;
        let mut universes = HashMap::new();
        let mut universe_data = HashMap::new();
        for port in ports {
            let ControllerPortAddress::E131Universe(universe) = port.address else {
                return Err(OutputError::Socket {
                    controller: id,
                    message: "E1.31 controller has a non-E1.31 port".to_string(),
                });
            };
            source
                .register_universe(universe)
                .map_err(|error| socket_error(&id, error))?;
            universes.insert(port.id, (universe, usize::from(port.slot_count)));
            universe_data.insert(port.id, vec![0; usize::from(port.slot_count) + 1]);
        }
        Ok(Self {
            id,
            source,
            termination_socket,
            config: config.clone(),
            universes,
            universe_data,
        })
    }

    pub fn send<'a>(
        &mut self,
        frames: impl IntoIterator<Item = &'a ControllerPortFrame>,
    ) -> Result<(), OutputError> {
        let destination = self.destination();
        for frame in frames {
            let (universe, expected) =
                self.universes.get(&frame.port).copied().ok_or_else(|| {
                    OutputError::MissingPort {
                        controller: self.id.clone(),
                        port: frame.port,
                    }
                })?;
            if frame.slots.len() != expected {
                return Err(OutputError::InvalidFrameLength {
                    controller: self.id.clone(),
                    expected,
                    actual: frame.slots.len(),
                });
            }
            let data = self.universe_data.get_mut(&frame.port).ok_or_else(|| {
                OutputError::MissingPort {
                    controller: self.id.clone(),
                    port: frame.port,
                }
            })?;
            data[1..].copy_from_slice(&frame.slots);
            self.source
                .send(
                    &[universe],
                    data,
                    Some(self.config.priority),
                    destination,
                    None,
                )
                .map_err(|error| socket_error(&self.id, error))?;
        }
        Ok(())
    }

    pub fn blackout(&mut self) -> Result<(), OutputError> {
        let frames = self
            .universes
            .iter()
            .map(|(port, (_, slots))| ControllerPortFrame {
                controller: self.id.clone(),
                port: *port,
                slots: vec![0; *slots],
            })
            .collect::<Vec<_>>();
        self.send(&frames)
    }

    pub fn terminate(&mut self) -> Result<(), OutputError> {
        let universes = self
            .universes
            .values()
            .map(|(universe, _)| *universe)
            .collect::<Vec<_>>();
        for universe in universes {
            match self.config.mode {
                E131Mode::Unicast { destination } => {
                    let destination = SocketAddr::new(destination, ACN_SDT_MULTICAST_PORT);
                    for sequence_number in 0..3 {
                        let packet = AcnRootLayerProtocol {
                            pdu: E131RootLayer {
                                cid: self
                                    .source
                                    .cid()
                                    .map_err(|error| socket_error(&self.id, error))?,
                                data: E131RootLayerData::DataPacket(DataPacketFramingLayer {
                                    source_name: self.config.source_name.clone().into(),
                                    priority: self.config.priority,
                                    synchronization_address: 0,
                                    sequence_number,
                                    preview_data: false,
                                    stream_terminated: true,
                                    force_synchronization: false,
                                    universe,
                                    data: DataPacketDmpLayer {
                                        property_values: vec![0].into(),
                                    },
                                }),
                            },
                        };
                        let bytes = packet
                            .pack_alloc()
                            .map_err(|error| socket_error(&self.id, error))?;
                        self.termination_socket
                            .send_to(&bytes, destination)
                            .map_err(|error| socket_error(&self.id, error))?;
                    }
                }
                E131Mode::Multicast => self
                    .source
                    .terminate_stream(universe, 0)
                    .map_err(|error| socket_error(&self.id, error))?,
            }
        }
        Ok(())
    }

    fn destination(&self) -> Option<SocketAddr> {
        match self.config.mode {
            E131Mode::Multicast => None,
            E131Mode::Unicast { destination } => {
                Some(SocketAddr::new(destination, ACN_SDT_MULTICAST_PORT))
            }
        }
    }
}

fn socket_error(id: &ControllerId, error: impl std::fmt::Debug) -> OutputError {
    OutputError::Socket {
        controller: id.clone(),
        message: format!("{error:?}"),
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, UdpSocket};
    use std::time::Duration;

    use donder_language::controller::{ControllerPort, ControllerPortAddress};
    use donder_language::identity::{DocumentId, SourceIdentity};
    use sacn::packet::{AcnRootLayerProtocol, E131RootLayerData};
    use uuid::Uuid;

    use super::*;

    #[test]
    fn unicast_loopback_has_expected_fields_sequence_and_blackout() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, ACN_SDT_MULTICAST_PORT)).unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let id = ControllerId(SourceIdentity::from_document(
            DocumentId::new(Uuid::new_v4(), "controller.donder".into()),
            "e131".to_string(),
        ));
        let config = E131Config {
            source_name: "Donder test".to_string(),
            bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            priority: 120,
            mode: E131Mode::Unicast {
                destination: IpAddr::V4(Ipv4Addr::LOCALHOST),
            },
        };
        let port = ControllerPort {
            id: ControllerPortId(1),
            address: ControllerPortAddress::E131Universe(42),
            slot_count: 3,
        };
        let mut sender = E131Sender::open(id.clone(), &config, &[port]).unwrap();
        let frame = ControllerPortFrame {
            controller: id,
            port: ControllerPortId(1),
            slots: vec![5, 6, 7],
        };
        sender.send(std::slice::from_ref(&frame)).unwrap();
        let first = receive_data(&receiver);
        sender.send(&[frame]).unwrap();
        let second = receive_data(&receiver);
        assert_eq!(first.source_name, "Donder test");
        assert_eq!(first.priority, 120);
        assert_eq!(first.universe, 42);
        assert_eq!(&first.data.property_values[..], &[0, 5, 6, 7]);
        assert_eq!(
            second.sequence_number,
            first.sequence_number.wrapping_add(1)
        );
        sender.blackout().unwrap();
        let blackout = receive_data(&receiver);
        assert_eq!(&blackout.data.property_values[..], &[0, 0, 0, 0]);
        sender.terminate().unwrap();
        for expected_sequence in 0..3 {
            let termination = receive_data(&receiver);
            assert!(termination.stream_terminated);
            assert_eq!(termination.sequence_number, expected_sequence);
            assert_eq!(&termination.data.property_values[..], &[0]);
        }
    }

    fn receive_data(socket: &UdpSocket) -> sacn::packet::DataPacketFramingLayer<'static> {
        let mut buffer = [0u8; 1024];
        let (length, _) = socket.recv_from(&mut buffer).unwrap();
        let packet = AcnRootLayerProtocol::parse(&buffer[..length]).unwrap();
        let E131RootLayerData::DataPacket(data) = packet.pdu.data else {
            panic!("expected E1.31 data packet");
        };
        sacn::packet::DataPacketFramingLayer {
            source_name: data.source_name.into_owned().into(),
            priority: data.priority,
            synchronization_address: data.synchronization_address,
            sequence_number: data.sequence_number,
            preview_data: data.preview_data,
            stream_terminated: data.stream_terminated,
            force_synchronization: data.force_synchronization,
            universe: data.universe,
            data: sacn::packet::DataPacketDmpLayer {
                property_values: data.data.property_values.into_owned().into(),
            },
        }
    }
}
