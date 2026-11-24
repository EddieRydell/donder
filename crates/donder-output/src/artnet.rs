use std::net::UdpSocket;

use crate::OutputFrame;
use artnet_protocol::{ArtCommand, Output, PaddedData, PortAddress};
use donder_model::{ArtNetConfig, ArtNetMode, ControllerId};

use crate::{OutputError, codec_error, socket_error};

pub struct ArtNetSender {
    id: ControllerId,
    socket: UdpSocket,
    config: ArtNetConfig,
    sequence: u8,
}

impl ArtNetSender {
    pub(crate) fn open(id: ControllerId, config: &ArtNetConfig) -> Result<Self, OutputError> {
        let socket = UdpSocket::bind(config.bind_address).map_err(|error| {
            socket_error(
                id,
                format_args!("could not open a socket on {}", config.bind_address),
                error,
            )
        })?;
        socket
            .set_broadcast(config.mode == ArtNetMode::Broadcast)
            .map_err(|error| {
                socket_error(
                    id,
                    format_args!("could not enable broadcast on {}", config.bind_address),
                    error,
                )
            })?;
        Ok(Self {
            id,
            socket,
            config: config.clone(),
            sequence: 1,
        })
    }

    pub(crate) fn send<'a>(
        &mut self,
        frames: impl IntoIterator<Item = &'a OutputFrame>,
    ) -> Result<(), OutputError> {
        let expected = self.config.universes.channel_count() as usize;
        for frame in frames {
            if frame.slots.len() != expected {
                return Err(OutputError::InvalidFrameLength {
                    controller: self.id,
                    expected,
                    actual: frame.slots.len(),
                });
            }
            for (address, channels) in self.config.universes.iter() {
                let command = ArtCommand::Output(Output {
                    sequence: self.sequence,
                    physical: 0,
                    port_address: PortAddress::try_from(address).map_err(|error| {
                        codec_error(
                            self.id,
                            format_args!(
                                "could not address Art-Net universe {address} for {}",
                                self.config.destination
                            ),
                            error,
                        )
                    })?,
                    data: PaddedData::from(
                        frame.slots[channels.start as usize..channels.end as usize].to_vec(),
                    ),
                    ..Output::default()
                });
                let bytes = command.write_to_buffer().map_err(|error| {
                    codec_error(
                        self.id,
                        format_args!(
                            "could not encode an Art-Net packet for {}",
                            self.config.destination
                        ),
                        error,
                    )
                })?;
                self.socket
                    .send_to(&bytes, self.config.destination)
                    .map_err(|error| {
                        socket_error(
                            self.id,
                            format_args!("could not send to {}", self.config.destination),
                            error,
                        )
                    })?;
            }
        }
        self.sequence = if self.sequence == u8::MAX {
            1
        } else {
            self.sequence + 1
        };
        Ok(())
    }

    pub(crate) fn blackout(&mut self) -> Result<(), OutputError> {
        self.send(&[OutputFrame {
            controller: self.id,
            slots: vec![0; self.config.universes.channel_count() as usize],
        }])
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    use artnet_protocol::ArtCommand;
    use donder_model::{ArtNetMode, Universes};

    use super::*;

    #[test]
    fn loopback_packets_increment_sequence_and_blackout() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let destination = receiver.local_addr().unwrap();
        let id = ControllerId(0);
        let config = ArtNetConfig {
            bind_address: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            destination,
            mode: ArtNetMode::Unicast,
            universes: Universes {
                start: 0x1234,
                count: 1,
                channels: 3,
            },
            fseq_channel: None,
            ports: Vec::new(),
        };
        let mut sender = ArtNetSender::open(id, &config).unwrap();
        let frame = OutputFrame {
            controller: id,
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
