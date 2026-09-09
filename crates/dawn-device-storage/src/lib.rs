#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use littlefs2::io::{Read, Write};
pub use littlefs2::{driver::Storage, io::Error};
use littlefs2::{fs::Filesystem, path, path::Path};

pub mod credentials;

pub use littlefs2::consts;

#[derive(Clone, Copy)]
pub enum Record {
    Credentials,
    Sequence,
}

impl Record {
    fn path(self) -> &'static Path {
        match self {
            Self::Credentials => path!("credentials"),
            Self::Sequence => path!("sequence"),
        }
    }
}

/// Format only a completely erased partition. Mount errors never erase data.
pub fn initialize<S: Storage>(storage: &mut S) -> Result<(), Error> {
    let mut bytes = [0; 256];
    if S::READ_SIZE == 0
        || bytes.len() % S::READ_SIZE != 0
        || (S::BLOCK_COUNT * S::BLOCK_SIZE) % bytes.len() != 0
    {
        return Err(Error::INVALID);
    }
    let mut erased = true;
    for offset in (0..S::BLOCK_COUNT * S::BLOCK_SIZE).step_by(bytes.len()) {
        storage.read(offset, &mut bytes)?;
        if bytes.iter().any(|&byte| byte != 0xff) {
            erased = false;
            break;
        }
    }
    if erased {
        Filesystem::format(storage)?;
    }
    Filesystem::mount_and_then(storage, |_| Ok(()))
}

/// Explicitly discard all records, including damaged filesystems.
/// The caller must obtain the operator's erase confirmation first.
pub fn erase_all<S: Storage>(storage: &mut S) -> Result<(), Error> {
    for offset in (0..S::BLOCK_COUNT * S::BLOCK_SIZE).step_by(S::BLOCK_SIZE) {
        storage.erase(offset, S::BLOCK_SIZE)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

pub fn read<S: Storage>(
    storage: &mut S,
    record: Record,
    limit: usize,
) -> Result<Option<Vec<u8>>, Error> {
    Filesystem::mount_and_then(storage, |fs| {
        let metadata = match fs.metadata(record.path()) {
            Ok(metadata) => metadata,
            Err(Error::NO_SUCH_ENTRY) => return Ok(None),
            Err(error) => return Err(error),
        };
        let length = metadata.len().checked_sub(4).ok_or(Error::CORRUPTION)?;
        if length > limit {
            return Err(Error::FILE_TOO_BIG);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| Error::NO_MEMORY)?;
        bytes.resize(length, 0);
        fs.open_file_and_then(record.path(), |file| {
            let mut checksum = [0; 4];
            file.read_exact(&mut checksum)?;
            let mut offset = 0;
            while offset < length {
                let read = file.read(&mut bytes[offset..])?;
                if read == 0 {
                    return Err(Error::IO);
                }
                offset += read;
            }
            if crc32fast::hash(&bytes) != u32::from_le_bytes(checksum) {
                return Err(Error::CORRUPTION);
            }
            Ok(())
        })?;
        Ok(Some(bytes))
    })
}

/// littlefs commits the closed temporary file before atomically replacing the record.
pub fn write<S: Storage>(storage: &mut S, record: Record, bytes: &[u8]) -> Result<(), Error> {
    Filesystem::mount_and_then(storage, |fs| {
        fs.create_file_and_then(path!("pending"), |file| {
            file.write_all(&crc32fast::hash(bytes).to_le_bytes())?;
            file.write_all(bytes)
        })?;
        fs.rename(path!("pending"), record.path())
    })
}
