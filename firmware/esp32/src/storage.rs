use donder_device_storage::{Error, Storage, consts};
use esp_bootloader_esp_idf::partitions::{PARTITION_TABLE_MAX_LEN, read_partition_table};
use esp_storage::{Flash, FlashStorage};

static FLASH_REQUESTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static FLASH_READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// The rendering core waits in an IRAM loop until this is dropped.
pub(crate) struct OutputSuspension;

impl Drop for OutputSuspension {
    fn drop(&mut self) {
        FLASH_REQUESTED.store(false, core::sync::atomic::Ordering::Release);
    }
}

/// Keep the rendering core in an interrupt-free IRAM loop while flash is used.
/// Hardware parking alone can freeze an outstanding cache fill indefinitely.
pub(crate) async fn suspend_output() -> OutputSuspension {
    use core::sync::atomic::Ordering::{Acquire, Release};
    // A previous suspension must finish its exit before a new request can
    // trust the acknowledgment for this one.
    while FLASH_READY.load(Acquire) {
        embassy_time::Timer::after_micros(100).await;
    }
    FLASH_REQUESTED.store(true, Release);
    while !FLASH_READY.load(Acquire) {
        embassy_time::Timer::after_micros(100).await;
    }
    OutputSuspension
}

pub(crate) fn flash_requested() -> bool {
    FLASH_REQUESTED.load(core::sync::atomic::Ordering::Acquire)
}

#[esp_hal::ram]
pub(crate) fn flash_checkpoint() {
    use core::sync::atomic::Ordering::{Acquire, Release};
    if !FLASH_REQUESTED.load(Acquire) {
        return;
    }
    let previous_ps: u32;
    // Safety: called on core 1 between completed DMA transfers, with no mutex
    // guard held. The entire wait is IRAM and must not enter a flash-resident ISR.
    unsafe { core::arch::asm!("rsil {0}, 5", out(reg) previous_ps, options(nostack)) };
    FLASH_READY.store(true, Release);
    while FLASH_REQUESTED.load(Acquire) {
        core::hint::spin_loop();
    }
    FLASH_READY.store(false, Release);
    unsafe { core::arch::asm!("wsr.ps {0}", "rsync", in(reg) previous_ps, options(nostack)) };
}

pub(crate) struct DeviceStorage {
    flash: FlashStorage<'static>,
    offset: u32,
    show_offset: u32,
}

// LittleFS provides byte buffers; the SDK low-level reads require word alignment.
// read_nor reserves a 4 KiB frame even on its aligned path in this build.
#[repr(align(4))]
struct AlignedBytes([u8; 256]);

impl DeviceStorage {
    pub(crate) fn new(peripheral: Flash<'static>) -> Result<Self, &'static str> {
        let mut flash = FlashStorage::new(peripheral).multicore_auto_park();
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
        Ok(Self {
            offset: partition("donder", Self::BLOCK_COUNT * Self::BLOCK_SIZE)
                .ok_or("Install the controller show with its Donder data partition")?,
            show_offset: partition("shows", donder_device_storage::show_slots::PARTITION_BYTES)
                .ok_or("Install the controller show with its show partition")?,
            flash,
        })
    }

    fn address(&self, offset: usize, length: usize) -> Result<u32, Error> {
        if offset
            .checked_add(length)
            .is_none_or(|end| end > Self::BLOCK_COUNT * Self::BLOCK_SIZE)
        {
            return Err(Error::INVALID);
        }
        Ok(self.offset + offset as u32)
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
        read_flash(self.address(offset, bytes.len())?, bytes)
    }
    fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<usize, Error> {
        let address = self.address(offset, bytes.len())?;
        write_flash(address, bytes)
    }
    fn erase(&mut self, offset: usize, length: usize) -> Result<usize, Error> {
        let address = self.address(offset, length)?;
        flash_critical(|| self.flash.erase(address, address + length as u32))
            .map_err(|_| Error::IO)?;
        Ok(length)
    }
}

impl DeviceStorage {
    pub(crate) fn shows(&mut self) -> Shows<'_> {
        Shows(self)
    }
    /// Read the slot's show into `bytes`, where playback reads it.
    pub(crate) fn read_show(
        &mut self,
        slot: donder_device_storage::show_slots::Slot,
        bytes: &mut [u8],
    ) -> Result<(), Error> {
        let mut shows = self.shows();
        for (index, chunk) in bytes.chunks_mut(1024).enumerate() {
            shows.read(slot.data_offset() + index * 1024, chunk)?;
        }
        Ok(())
    }
}

pub(crate) struct Shows<'a>(&'a mut DeviceStorage);
impl Shows<'_> {
    fn address(&self, offset: usize, length: usize) -> Result<u32, Error> {
        if offset
            .checked_add(length)
            .is_none_or(|end| end > donder_device_storage::show_slots::PARTITION_BYTES)
        {
            return Err(Error::INVALID);
        }
        Ok(self.0.show_offset + offset as u32)
    }
}
impl Storage for Shows<'_> {
    const READ_SIZE: usize = 4;
    const WRITE_SIZE: usize = 4;
    const BLOCK_SIZE: usize = 4096;
    const BLOCK_COUNT: usize = 64;
    const BLOCK_CYCLES: isize = -1;
    type CACHE_SIZE = consts::U256;
    type LOOKAHEAD_SIZE = consts::U1;
    fn read(&mut self, offset: usize, bytes: &mut [u8]) -> Result<usize, Error> {
        read_flash(self.address(offset, bytes.len())?, bytes)
    }
    fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<usize, Error> {
        let address = self.address(offset, bytes.len())?;
        write_flash(address, bytes)
    }
    fn erase(&mut self, offset: usize, length: usize) -> Result<usize, Error> {
        let address = self.address(offset, length)?;
        flash_critical(|| self.0.flash.erase(address, address + length as u32))
            .map_err(|_| Error::IO)?;
        Ok(length)
    }
}

fn read_flash(address: u32, bytes: &mut [u8]) -> Result<usize, Error> {
    // The ESP32 ROM reader also operates on the shared flash/cache hardware.
    // Protect it just like writes, including slot checksums while rendering.
    flash_critical(|| with_other_core_parked(|| read_flash_parked(address, bytes)))
}

fn read_flash_parked(address: u32, bytes: &mut [u8]) -> Result<usize, Error> {
    let mut scratch = AlignedBytes([0; 256]);
    for (index, chunk) in bytes.chunks_mut(256).enumerate() {
        // Safety: callers validated partition bounds, and both the scratch buffer
        // and each read's address/length satisfy the SDK's word alignment rules.
        unsafe {
            esp_storage::ll::spiflash_read(
                address + (index * 256) as u32,
                scratch.0.as_mut_ptr().cast(),
                chunk.len() as u32,
            )
        }
        .map_err(|_| Error::IO)?;
        chunk.copy_from_slice(&scratch.0[..chunk.len()]);
    }
    Ok(bytes.len())
}
fn write_flash(address: u32, bytes: &[u8]) -> Result<usize, Error> {
    // Acquire the cross-core critical section before parking: a frozen core
    // must not own that lock, and the scheduler must not run while it is frozen.
    flash_critical(|| write_flash_parked(address, bytes))
}

fn flash_critical<T>(operation: impl FnOnce() -> T) -> T {
    embassy_sync::blocking_mutex::raw::RawMutex::lock(
        &embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex::new(),
        operation,
    )
}

fn write_flash_parked(address: u32, bytes: &[u8]) -> Result<usize, Error> {
    // Use the SDK's same automatic parking policy with its aligned low-level
    // writer. write_nor reserves a 4-KiB scratch frame even for aligned inputs.
    with_other_core_parked(|| {
        // Safety: callers validated bounds; these existing SDK routines provide
        // their normal interrupt protection. The other core is parked above.
        unsafe { esp_storage::ll::spiflash_unlock() }.map_err(|_| Error::IO)?;
        let mut scratch = AlignedBytes([0; 256]);
        for (index, chunk) in bytes.chunks(256).enumerate() {
            scratch.0[..chunk.len()].copy_from_slice(chunk);
            unsafe {
                esp_storage::ll::spiflash_write(
                    address + (index * 256) as u32,
                    scratch.0.as_ptr().cast(),
                    chunk.len() as u32,
                )
            }
            .map_err(|_| Error::IO)?;
        }
        Ok(bytes.len())
    })
}

// Call only while the cross-core critical section is held.
fn with_other_core_parked<T>(operation: impl FnOnce() -> T) -> T {
    use esp_hal::system::{Cpu, CpuControl, is_running};
    let mut cpu = CpuControl::new(unsafe { esp_hal::peripherals::CPU_CTRL::steal() });
    let other = Cpu::other().find(|&core| is_running(core));
    if let Some(other) = other {
        // Safety: Cpu::other excludes the executing core; storage has one owner.
        unsafe { cpu.park_core(other) };
    }
    // Hardware-stalling a core does not stop its outstanding cache fill.
    // Wait for and disable that cache before ROM SPI operations disturb the
    // shared flash hardware; restore it before allowing the core to continue.
    let app_cache = unsafe { disable_app_cache() };
    let result = operation();
    unsafe { restore_app_cache(app_cache) };
    if let Some(other) = other {
        cpu.unpark_core(other);
    }
    result
}

#[esp_hal::ram]
unsafe fn disable_app_cache() -> bool {
    unsafe extern "C" {
        fn Cache_Read_Disable_rom(cpu: u32);
    }
    let enabled = esp_hal::peripherals::DPORT::regs()
        .app_cache_ctrl()
        .read()
        .app_cache_enable()
        .bit();
    if enabled {
        unsafe { Cache_Read_Disable_rom(1) };
    }
    enabled
}

#[esp_hal::ram]
unsafe fn restore_app_cache(enabled: bool) {
    unsafe extern "C" {
        fn Cache_Flush_rom(cpu: u32);
        fn Cache_Read_Enable_rom(cpu: u32);
    }
    if enabled {
        unsafe {
            Cache_Flush_rom(1);
            Cache_Read_Enable_rom(1);
        }
    }
}
