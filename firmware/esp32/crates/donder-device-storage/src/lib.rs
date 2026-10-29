#![no_std]

extern crate alloc;

#[cfg(test)]
#[path = "../../../src/transport.rs"]
mod transport;

#[cfg(test)]
mod protocol_tests;

use alloc::vec::Vec;
pub use littlefs2::{driver::Storage, io::Error};
use littlefs2::{fs::Filesystem, path, path::Path};

pub mod device_config;
pub mod show_slots;

pub use littlefs2::consts;

/// The device record's file; a fresh controller has none.
const DEVICE: &Path = path!("device");
/// The longest device record.
const DEVICE_BYTES: usize = 1024;

/// Format only a completely erased partition. Mount errors never erase data.
pub fn initialize<S: Storage>(storage: &mut S) -> Result<(), Error> {
    let mut bytes = [0; 256];
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

fn read<S: Storage>(storage: &mut S) -> Result<Option<Vec<u8>>, Error> {
    Filesystem::mount_and_then(storage, |fs| match fs.read::<DEVICE_BYTES>(DEVICE) {
        Ok(bytes) => Ok(Some(bytes.to_vec())),
        Err(Error::NO_SUCH_ENTRY) => Ok(None),
        Err(error) => Err(error),
    })
}

/// littlefs commits the closed temporary file before atomically replacing the record.
fn write<S: Storage>(storage: &mut S, bytes: &[u8]) -> Result<(), Error> {
    Filesystem::mount_and_then(storage, |fs| {
        fs.write(path!("pending"), bytes)?;
        fs.rename(path!("pending"), DEVICE)
    })
}
