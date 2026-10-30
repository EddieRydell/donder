use core::{
    ops::{Deref, DerefMut},
    sync::atomic::{
        AtomicBool,
        Ordering::{Acquire, Release},
    },
};
use donder_device_storage::{Error, Storage, consts};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    mutex::{Mutex, MutexGuard},
};
use esp_bootloader_esp_idf::partitions::{PARTITION_TABLE_MAX_LEN, read_partition_table};
use esp_storage::{Flash, FlashStorage};

// On the ESP32 neither core may run code or read data from flash while flash
// is read, written or erased, so the rendering core waits in RAM for each
// storage access.
static PAUSE_REQUESTED: AtomicBool = AtomicBool::new(false);
static PAUSED: AtomicBool = AtomicBool::new(false);

pub(crate) type SharedStorage = Mutex<CriticalSectionRawMutex, DeviceStorage>;

/// Storage, usable while the rendering core waits in RAM.
pub(crate) struct Access<'a>(MutexGuard<'a, CriticalSectionRawMutex, DeviceStorage>);

/// Pause the rendering core, then lend the storage.
pub(crate) async fn access(storage: &SharedStorage) -> Access<'_> {
    let guard = storage.lock().await;
    // The previous access ends only once the rendering core has left its wait.
    while PAUSED.load(Acquire) {
        embassy_time::Timer::after_micros(100).await;
    }
    PAUSE_REQUESTED.store(true, Release);
    while !PAUSED.load(Acquire) {
        embassy_time::Timer::after_micros(100).await;
    }
    Access(guard)
}

impl Drop for Access<'_> {
    fn drop(&mut self) {
        PAUSE_REQUESTED.store(false, Release);
    }
}

impl Deref for Access<'_> {
    type Target = DeviceStorage;
    fn deref(&self) -> &DeviceStorage {
        &self.0
    }
}

impl DerefMut for Access<'_> {
    fn deref_mut(&mut self) -> &mut DeviceStorage {
        &mut self.0
    }
}

/// Called by the rendering core between frames, holding no locks.
pub(crate) fn pause_requested() -> bool {
    PAUSE_REQUESTED.load(Acquire)
}

/// Wait in RAM until the access ends. The rendering core takes no
/// interrupts, so nothing on it reads flash meanwhile.
#[esp_hal::ram]
pub(crate) fn pause() {
    PAUSED.store(true, Release);
    while PAUSE_REQUESTED.load(Acquire) {
        core::hint::spin_loop();
    }
    PAUSED.store(false, Release);
}

/// The Donder data and show partitions.
pub(crate) struct DeviceStorage {
    flash: FlashStorage<'static>,
    offset: u32,
    show_offset: u32,
}

impl DeviceStorage {
    pub(crate) fn new(peripheral: Flash<'static>) -> Result<Self, &'static str> {
        // Safety: the rendering core starts after boot storage use and waits
        // in RAM through every later access.
        let mut flash = unsafe { FlashStorage::new(peripheral).multicore_ignore() };
        let mut bytes = alloc::vec![0; PARTITION_TABLE_MAX_LEN];
        let table =
            read_partition_table(&mut flash, &mut bytes).map_err(|_| "Invalid partition table")?;
        // The build flashes this table; only a different firmware's table lacks these.
        let partition = |label: &str, len: usize| {
            table
                .iter()
                .find(|entry| entry.label_as_str() == label && entry.len() as usize == len)
                .map(|entry| entry.offset())
        };
        let offset = partition("donder", Self::BLOCK_COUNT * Self::BLOCK_SIZE)
            .ok_or("Install the controller firmware with its Donder data partition")?;
        let show_offset = partition("shows", donder_device_storage::show_slots::PARTITION_BYTES)
            .ok_or("Install the controller firmware with its show partition")?;
        Ok(Self {
            flash,
            offset,
            show_offset,
        })
    }

    pub(crate) fn shows(&mut self) -> Shows<'_> {
        Shows(self)
    }

    /// Read the slot's show into `bytes`, where playback reads it.
    pub(crate) fn read_show(
        &mut self,
        slot: donder_device_storage::show_slots::Slot,
        bytes: &mut [u8],
    ) -> Result<(), Error> {
        self.shows().read(slot.data_offset(), bytes).map(|_| ())
    }

    fn read_at(&mut self, address: u32, bytes: &mut [u8]) -> Result<usize, Error> {
        self.flash.read_nor(address, bytes).map_err(|_| Error::IO)?;
        Ok(bytes.len())
    }

    fn write_at(&mut self, address: u32, bytes: &[u8]) -> Result<usize, Error> {
        self.flash
            .write_nor(address, bytes)
            .map_err(|_| Error::IO)?;
        Ok(bytes.len())
    }

    fn erase_at(&mut self, address: u32, length: usize) -> Result<usize, Error> {
        self.flash
            .erase(address, address + length as u32)
            .map_err(|_| Error::IO)?;
        Ok(length)
    }
}

impl Storage for DeviceStorage {
    const READ_SIZE: usize = 4;
    const WRITE_SIZE: usize = 4;
    const BLOCK_SIZE: usize = 4096;
    const BLOCK_COUNT: usize = 64;
    const BLOCK_CYCLES: isize = 500;
    type CACHE_SIZE = consts::U256;
    type LOOKAHEAD_SIZE = consts::U1;

    fn read(&mut self, offset: usize, bytes: &mut [u8]) -> Result<usize, Error> {
        self.read_at(self.offset + offset as u32, bytes)
    }
    fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<usize, Error> {
        self.write_at(self.offset + offset as u32, bytes)
    }
    fn erase(&mut self, offset: usize, length: usize) -> Result<usize, Error> {
        self.erase_at(self.offset + offset as u32, length)
    }
}

pub(crate) struct Shows<'a>(&'a mut DeviceStorage);

impl Storage for Shows<'_> {
    const READ_SIZE: usize = 4;
    const WRITE_SIZE: usize = 4;
    const BLOCK_SIZE: usize = 4096;
    const BLOCK_COUNT: usize = 64;
    const BLOCK_CYCLES: isize = -1;
    type CACHE_SIZE = consts::U256;
    type LOOKAHEAD_SIZE = consts::U1;

    fn read(&mut self, offset: usize, bytes: &mut [u8]) -> Result<usize, Error> {
        self.0.read_at(self.0.show_offset + offset as u32, bytes)
    }
    fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<usize, Error> {
        self.0.write_at(self.0.show_offset + offset as u32, bytes)
    }
    fn erase(&mut self, offset: usize, length: usize) -> Result<usize, Error> {
        self.0.erase_at(self.0.show_offset + offset as u32, length)
    }
}
