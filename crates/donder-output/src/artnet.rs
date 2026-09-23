use std::collections::HashMap;
use std::net::UdpSocket;

use artnet_protocol::{ArtCommand, Output, PaddedData, PortAddress};
use donder_elaboration::ControllerPortFrame;
use donder_language::controller::{
    ArtNetConfig, ArtNetMode, ControllerId, ControllerPort, ControllerPortAddress, ControllerPortId,
};

use crate::OutputError;

pub struct ArtNetSender {
    id: ControllerId,
    socket: UdpSocket,
    config: ArtNetConfig,
    ports: HashMap<ControllerPortId, (u16, usize)>,
    sequence: u8,
}

impl ArtNetSender {
    pub fn open(
        id: ControllerId,
        config: &ArtNetConfig,
        ports: &[ControllerPort],
    ) -> Result<Self, OutputError> {
        let socket =
            UdpSocket::bind(config.bind_address).map_err(|error| socket_error(&id, error))?;
        socket
            .set_broadcast(config.mode == ArtNetMode::Broadcast)
            .map_err(|error| socket_error(&id, error))?;
        let mut mapped = HashMap::new();
        for port in ports {
            let ControllerPortAddress::ArtNetPort(address) = port.address else {
                return Err(OutputError::Socket {
                    controller: id,
                    message: "Art-Net controller has a non-Art-Net port".to_string(),
                });
            };
            mapped.insert(port.id, (address, usize::from(port.slot_count)));
        }
        Ok(Self {
            id,
            socket,
            config: config.clone(),
            ports: mapped,
            sequence: 1,
        })
    }

    pub fn send<'a>(
        &mut self,
        frames: impl IntoIterator<Item = &'a ControllerPortFrame>,
    ) -> Result<(), OutputError> {
        for frame in frames {
            let (address, expected) =
                self.ports
                    .get(&frame.port)
                    .copied()
                    .ok_or_else(|| OutputError::MissingPort {
                        controller: self.id.clone(),
                        port: frame.port,
                    })?;
            if frame.slots.len() != expected {
                return Err(OutputError::InvalidFrameLength {
                    controller: self.id.clone(),
                    expected,
                    actual: frame.slots.len(),
                });
            }
            let command = ArtCommand::Output(Output {
                sequence: self.sequence,
                physical: 0,
                port_address: PortAddress::try_from(address)
                    .map_err(|error| codec_error(&self.id, error))?,
                data: PaddedData::from(frame.slots.clone()),
                ..Output::default()
            });
            let bytes = command
                .write_to_buffer()
                .map_err(|error| codec_error(&self.id, error))?;
            self.socket
                .send_to(&bytes, self.config.destination)
                .map_err(|error| socket_error(&self.id, error))?;
        }
        self.sequence = if self.sequence == u8::MAX {
            1
        } else {
            self.sequence + 1
        };
        Ok(())
    }

    pub fn blackout(&mut self) -> Result<(), OutputError> {
        let frames = self
            .ports
            .iter()
            .map(|(port, (_, slots))| ControllerPortFrame {
                controller: self.id.clone(),
                port: *port,
                slots: vec![0; *slots],
            })
            .collect::<Vec<_>>();
        self.send(&frames)
    }
}

fn socket_error(id: &ControllerId, error: impl std::fmt::Display) -> OutputError {
    OutputError::Socket {
        controller: id.clone(),
        message: error.to_string(),
    }
}
fn codec_error(id: &ControllerId, error: impl std::fmt::Debug) -> OutputError {
    OutputError::Codec {
        controller: id.clone(),
        message: format!("{error:?}"),
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    use artnet_protocol::ArtCommand;
    use donder_language::controller::{ArtNetMode, ControllerPort, ControllerPortAddress};
    use donder_language::identity::{DocumentId, SourceIdentity};
    use uuid::Uuid;

    use super::*;

    #[test]
    fn loopback_packets_increment_sequence_and_blackout() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let destination = receiver.local_addr().unwrap();
        let id = ControllerId(SourceIdentity::from_document(
            DocumentId::new(Uuid::new_v4(), "controller.donder".into()),
            "artnet".to_string(),
        ));
        let config = ArtNetConfig {
            bind_address: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            destination,
            mode: ArtNetMode::Unicast,
        };
        let port = ControllerPort {
            id: ControllerPortId(1),
            address: ControllerPortAddress::ArtNetPort(0x1234),
            slot_count: 3,
        };
        let mut sender = ArtNetSender::open(id.clone(), &config, &[port]).unwrap();
        let frame = ControllerPortFrame {
            controller: id,
            port: ControllerPortId(1),
            slots: vec![1, 2, 3],
        };
        sender.send(std::slice::from_ref(&frame)).unwrap();
        let first = receive_output(&receiver);
        sender.send(&[frame]).unwrap();
        let second = receive_output(&receiver);
        assert_eq!(first.sequence, 1);
        assert_eq!(second.sequence, 2);
        assert_eq!(first.port_address, PortAddress::try_from(0x1234).unwrap());
        assert_eq!(&first.data.as_ref()[..3], &[1, 2, 3]);
        sender.blackout().unwrap();
        let blackout = receive_output(&receiver);
        assert_eq!(&blackout.data.as_ref()[..3], &[0, 0, 0]);
    }

    fn receive_output(socket: &UdpSocket) -> Output {
        let mut buffer = [0u8; 1024];
        let (length, _) = socket.recv_from(&mut buffer).unwrap();
        let ArtCommand::Output(output) = ArtCommand::from_buffer(&buffer[..length]).unwrap() else {
            panic!("expected ArtDmx");
        };
        output
    }
}
