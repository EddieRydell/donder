//! The UDP clock exchange. The editor sends its token and send time; the
//! controller echoes the send time with its boot and the times it received
//! the request and replied, so four timestamps measure offset and delay.

const MAGIC: [u8; 4] = *b"DCLK";

/// The claim token, as its hex text.
pub type Token = [u8; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub token: Token,
    /// The editor's clock when it sent the request.
    pub sent_micros: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    /// The request's send time, echoed.
    pub sent_micros: u64,
    pub boot_id: u32,
    /// The controller's clock when the request arrived, and when it replied.
    pub received_micros: u64,
    pub replied_micros: u64,
}

impl Request {
    pub const BYTES: usize = 44;

    pub fn encode(&self) -> [u8; Self::BYTES] {
        let mut bytes = [0; Self::BYTES];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4..36].copy_from_slice(&self.token);
        bytes[36..].copy_from_slice(&self.sent_micros.to_le_bytes());
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; Self::BYTES] = bytes.try_into().ok()?;
        (bytes[..4] == MAGIC).then(|| Self {
            token: field(bytes, 4),
            sent_micros: u64::from_le_bytes(field(bytes, 36)),
        })
    }
}

impl Reply {
    pub const BYTES: usize = 32;

    pub fn encode(&self) -> [u8; Self::BYTES] {
        let mut bytes = [0; Self::BYTES];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4..12].copy_from_slice(&self.sent_micros.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.boot_id.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.received_micros.to_le_bytes());
        bytes[24..].copy_from_slice(&self.replied_micros.to_le_bytes());
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; Self::BYTES] = bytes.try_into().ok()?;
        (bytes[..4] == MAGIC).then(|| Self {
            sent_micros: u64::from_le_bytes(field(bytes, 4)),
            boot_id: u32::from_le_bytes(field(bytes, 12)),
            received_micros: u64::from_le_bytes(field(bytes, 16)),
            replied_micros: u64::from_le_bytes(field(bytes, 24)),
        })
    }
}

/// The `N` bytes at `at`.
fn field<const N: usize>(bytes: &[u8], at: usize) -> [u8; N] {
    let mut field = [0; N];
    field.copy_from_slice(&bytes[at..at + N]);
    field
}
