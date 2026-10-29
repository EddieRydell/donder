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

/// Mount the partition, formatting it when it holds no usable filesystem; the
/// editor recreates every record it holds.
pub fn initialize<S: Storage>(storage: &mut S) -> Result<(), Error> {
    if Filesystem::mount_and_then(storage, |_| Ok(())).is_err() {
        Filesystem::format(storage)?;
    }
    Filesystem::mount_and_then(storage, |_| Ok(()))
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
