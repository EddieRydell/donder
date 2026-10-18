//! Two contiguous flash slots. Payloads stay memory-mappable for playback.
use crate::{Error, Storage};

pub const SLOT_BYTES: usize = 128 * 1024;
pub const DATA_OFFSET: usize = 4096;
pub const PARTITION_BYTES: usize = 2 * SLOT_BYTES;
pub const MAX_PAYLOAD_BYTES: usize = 96 * 1024;
const MAGIC: &[u8; 4] = b"DSHW";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub index: usize,
    pub generation: u32,
    pub length: usize,
}

impl Slot {
    pub fn data_offset(self) -> usize {
        self.index * SLOT_BYTES + DATA_OFFSET
    }
}

pub fn latest<S: Storage>(storage: &mut S) -> Result<Option<Slot>, Error> {
    let mut selected: Option<Slot> = None;
    for index in 0..2 {
        let mut header = [0; 16];
        storage.read(index * SLOT_BYTES, &mut header)?;
        // A torn header is uncommitted; it cannot supersede the previous slot.
        if &header[..4] != MAGIC
            || crc32fast::hash(&header[..12])
                != u32::from_le_bytes(header[12..16].try_into().unwrap())
        {
            continue;
        }
        let slot = Slot {
            index,
            generation: u32::from_le_bytes(header[4..8].try_into().unwrap()),
            length: u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize,
        };
        if !(16..=16 + MAX_PAYLOAD_BYTES).contains(&slot.length) {
            return Err(Error::CORRUPTION);
        }
        if selected.is_none_or(|old| slot.generation.wrapping_sub(old.generation) as i32 > 0) {
            selected = Some(slot);
        }
    }
    Ok(selected)
}

pub fn begin<S: Storage>(storage: &mut S, length: usize) -> Result<Slot, Error> {
    if !(16..=16 + MAX_PAYLOAD_BYTES).contains(&length) {
        return Err(Error::FILE_TOO_BIG);
    }
    let old = latest(storage)?;
    let slot = Slot {
        index: old.map_or(0, |slot| 1 - slot.index),
        generation: old.map_or(1, |slot| slot.generation.wrapping_add(1)),
        length,
    };
    for offset in (0..SLOT_BYTES).step_by(S::BLOCK_SIZE) {
        storage.erase(slot.index * SLOT_BYTES + offset, S::BLOCK_SIZE)?;
    }
    Ok(slot)
}

pub fn append<S: Storage>(
    storage: &mut S,
    slot: Slot,
    offset: usize,
    bytes: &[u8],
) -> Result<(), Error> {
    if slot.index >= 2
        || !(16..=16 + MAX_PAYLOAD_BYTES).contains(&slot.length)
        || !offset.is_multiple_of(S::WRITE_SIZE)
        || !bytes.len().is_multiple_of(S::WRITE_SIZE)
        || offset
            .checked_add(bytes.len())
            .is_none_or(|end| end > slot.length.next_multiple_of(S::WRITE_SIZE))
    {
        return Err(Error::INVALID);
    }
    storage.write(slot.data_offset() + offset, bytes)?;
    Ok(())
}

/// Commit only after the complete archive is staged and loads.
pub fn commit<S: Storage>(storage: &mut S, slot: Slot) -> Result<(), Error> {
    if slot.index >= 2 || !(16..=16 + MAX_PAYLOAD_BYTES).contains(&slot.length) {
        return Err(Error::INVALID);
    }
    let mut header = [0; 16];
    header[..4].copy_from_slice(MAGIC);
    header[4..8].copy_from_slice(&slot.generation.to_le_bytes());
    header[8..12].copy_from_slice(&(slot.length as u32).to_le_bytes());
    let crc = crc32fast::hash(&header[..12]);
    header[12..16].copy_from_slice(&crc.to_le_bytes());
    storage.write(slot.index * SLOT_BYTES, &header)?;
    Ok(())
}
