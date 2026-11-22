use std::net::{IpAddr, SocketAddr, UdpSocket};

use crate::OutputFrame;
use donder_model::{ControllerId, E131Config, E131Mode};
use sacn::packet::{
    ACN_SDT_MULTICAST_PORT, AcnRootLayerProtocol, DataPacketDmpLayer, DataPacketFramingLayer,
    E131_TERMINATE_STREAM_PACKET_COUNT, E131RootLayer, E131RootLayerData, STARTING_SEQUENCE_NUMBER,
    universe_to_ipv4_multicast_addr, universe_to_ipv6_multicast_addr,
};
use uuid::Uuid;

use crate::OutputError;

/// Sends E1.31 data packets on one socket. The `sacn` crate's `SacnSource` is not used: its
/// discovery thread makes every drop wait up to a second, which stalls disabling output.
pub struct E131Sender {
    id: ControllerId,
    socket: UdpSocket,
    cid: Uuid,
    config: E131Config,
    /// Each universe's next sequence number, in the order `config.universes` lists them.
    sequences: Vec<u8>,
    /// One universe's data: the start code, then its channels.
    universe_data: Vec<u8>,
}

impl E131Sender {
    pub(crate) fn open(id: ControllerId, config: &E131Config) -> Result<Self, OutputError> {
        let socket = UdpSocket::bind(SocketAddr::new(config.bind_address, 0))
            .map_err(|error| socket_error(&id, error))?;
        Ok(Self {
            id,
            socket,
            cid: Uuid::new_v4(),
            config: config.clone(),
            sequences: vec![STARTING_SEQUENCE_NUMBER; usize::from(config.universes.count)],
            universe_data: vec![0; usize::from(config.universes.channels) + 1],
        })
    }

    /// Send each frame's channels, a universe at a time.
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
            for (index, (universe, channels)) in self.config.universes.iter().enumerate() {
                self.universe_data[1..]
                    .copy_from_slice(&frame.slots[channels.start as usize..channels.end as usize]);
                let sequence_number = self.next_sequence(index);
                self.send_packet(universe, sequence_number, &self.universe_data, false)?;
            }
        }
        Ok(())
    }

    pub(crate) fn blackout(&mut self) -> Result<(), OutputError> {
        self.send(&[OutputFrame {
            controller: self.id,
            slots: vec![0; self.config.universes.channel_count() as usize],
        }])
    }

    pub(crate) fn terminate(&mut self) -> Result<(), OutputError> {
        for (index, (universe, _)) in self.config.universes.iter().enumerate() {
            for _ in 0..E131_TERMINATE_STREAM_PACKET_COUNT {
                let sequence_number = self.next_sequence(index);
                self.send_packet(universe, sequence_number, &[0], true)?;
            }
        }
        Ok(())
    }

    fn next_sequence(&mut self, index: usize) -> u8 {
        let sequence_number = self.sequences[index];
        self.sequences[index] = sequence_number.wrapping_add(1);
        sequence_number
    }

    fn send_packet(
        &self,
        universe: u16,
        sequence_number: u8,
        data: &[u8],
        stream_terminated: bool,
    ) -> Result<(), OutputError> {
        let packet = AcnRootLayerProtocol {
            pdu: E131RootLayer {
                cid: self.cid,
                data: E131RootLayerData::DataPacket(DataPacketFramingLayer {
                    source_name: self.config.sender_name.as_str().into(),
                    priority: self.config.priority,
                    synchronization_address: 0,
                    sequence_number,
                    preview_data: false,
                    stream_terminated,
                    force_synchronization: false,
                    universe,
                    data: DataPacketDmpLayer {
                        property_values: data.into(),
                    },
                }),
            },
        };
        let bytes = packet
            .pack_alloc()
            .map_err(|error| socket_error(&self.id, error))?;
        let destination = self.destination(universe)?;
        self.socket
            .send_to(&bytes, destination)
            .map_err(|error| socket_error(&self.id, error))?;
        Ok(())
    }

    fn destination(&self, universe: u16) -> Result<SocketAddr, OutputError> {
        match self.config.mode {
            E131Mode::Unicast { destination } => {
                Ok(SocketAddr::new(destination, ACN_SDT_MULTICAST_PORT))
            }
            E131Mode::Multicast => {
                let address = match self.config.bind_address {
                    IpAddr::V4(_) => universe_to_ipv4_multicast_addr(universe),
                    IpAddr::V6(_) => universe_to_ipv6_multicast_addr(universe),
                }
                .map_err(|error| socket_error(&self.id, error))?;
                address
                    .as_socket()
                    .ok_or_else(|| socket_error(&self.id, "multicast address is not IP"))
            }
        }
    }
}

fn socket_error(id: &ControllerId, error: impl std::fmt::Debug) -> OutputError {
    OutputError::Socket {
        controller: *id,
        message: format!("{error:?}"),
    }
}
