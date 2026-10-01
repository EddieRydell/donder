use donder_device_storage::{Error, Storage, consts};
use esp_bootloader_esp_idf::partitions::{PARTITION_TABLE_MAX_LEN, read_partition_table};
use esp_storage::{Flash, FlashStorage};

#[cfg(feature = "i2s-output")]
static OUTPUT_RUNNING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "i2s-output")]
static FLASH_REQUESTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "i2s-output")]
static FLASH_READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub struct OutputSuspension {
    #[cfg(feature = "i2s-output")]
    active: bool,
}

impl Drop for OutputSuspension {
    fn drop(&mut self) {
        #[cfg(feature = "i2s-output")]
        if self.active {
            FLASH_REQUESTED.store(false, core::sync::atomic::Ordering::Release);
        }
    }
}

/// Keep the rendering core in an interrupt-free IRAM loop while flash is used.
/// Hardware parking alone can freeze an outstanding cache fill indefinitely.
pub async fn suspend_output() -> Result<OutputSuspension, embassy_time::TimeoutError> {
    #[cfg(feature = "i2s-output")]
    {
        use core::sync::atomic::Ordering::{Acquire, Release};
        if OUTPUT_RUNNING.load(Acquire) {
            let suspension = OutputSuspension { active: true };
            embassy_time::with_timeout(embassy_time::Duration::from_secs(2), async {
                // A previous suspension must finish its exit before a new
                // request can trust the acknowledgment for this suspension.
                while FLASH_READY.load(Acquire) {
                    embassy_time::Timer::after_micros(100).await;
                }
                FLASH_REQUESTED.store(true, Release);
                while !FLASH_READY.load(Acquire) {
                    embassy_time::Timer::after_micros(100).await;
                }
            })
            .await?;
            return Ok(suspension);
        }
    }
    Ok(OutputSuspension {
        #[cfg(feature = "i2s-output")]
        active: false,
    })
}

#[cfg(feature = "i2s-output")]
pub fn output_started() {
    OUTPUT_RUNNING.store(true, core::sync::atomic::Ordering::Release);
}

#[cfg(feature = "i2s-output")]
pub fn flash_requested() -> bool {
    FLASH_REQUESTED.load(core::sync::atomic::Ordering::Acquire)
}

#[cfg(feature = "i2s-output")]
#[esp_hal::ram]
pub fn flash_checkpoint() {
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

pub struct DeviceStorage {
    flash: FlashStorage<'static>,
    offset: u32,
    show_offset: u32,
}

// LittleFS provides byte buffers; the SDK low-level reads require word alignment.
// read_nor reserves a 4 KiB frame even on its aligned path in this build.
#[repr(align(4))]
struct AlignedBytes([u8; 256]);

impl DeviceStorage {
    pub fn new(peripheral: Flash<'static>) -> Result<Self, &'static str> {
        let mut flash = FlashStorage::new(peripheral).multicore_auto_park();
        let mut bytes = alloc::vec![0; PARTITION_TABLE_MAX_LEN];
        let table =
            read_partition_table(&mut flash, &mut bytes).map_err(|_| "Invalid partition table")?;
        let mut matches = table
            .iter()
            .filter(|entry| entry.label_as_str() == "donder");
        let entry = matches
            .next()
            .ok_or("Install the controller image with its Donder data partition")?;
        if matches.next().is_some()
            || entry.raw_type() != 1
            || entry.raw_subtype() != 6
            || entry.flags() != 0
            || entry.len() as usize != Self::BLOCK_COUNT * Self::BLOCK_SIZE
            || entry.offset() % Self::BLOCK_SIZE as u32 != 0
            || entry.offset() < 0x10000
        {
            return Err("Invalid Donder data partition");
        }
        let end = entry
            .offset()
            .checked_add(entry.len())
            .ok_or("Invalid Donder partition bounds")?;
        if end as usize > flash.capacity()
            || table.iter().any(|other| {
                other.label_as_str() != "donder"
                    && other.offset() < end
                    && other.offset().saturating_add(other.len()) > entry.offset()
            })
        {
            return Err("Donder data partition overlaps another partition or exceeds flash");
        }
        let mut show_entries = table.iter().filter(|entry| entry.label_as_str() == "shows");
        let shows = show_entries.next().ok_or("Missing show partition")?;
        if show_entries.next().is_some()
            || shows.raw_type() != 1
            || shows.raw_subtype() != 6
            || shows.flags() != 0
            || shows.len() as usize != donder_device_storage::show_slots::PARTITION_BYTES
            || shows.offset() % 65536 != 0
            || shows.offset() < 0x10000
            || shows
                .offset()
                .checked_add(shows.len())
                .is_none_or(|end| end as usize > flash.capacity())
            || table.iter().any(|other| {
                other.label_as_str() != "shows"
                    && other.offset() < shows.offset() + shows.len()
                    && other.offset().saturating_add(other.len()) > shows.offset()
            })
        {
            return Err("Invalid show partition");
        }
        map_shows(shows.offset())?;
        Ok(Self {
            flash,
            offset: entry.offset(),
            show_offset: shows.offset(),
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
        write_flash(&mut self.flash, address, bytes)
    }
    fn erase(&mut self, offset: usize, length: usize) -> Result<usize, Error> {
        let address = self.address(offset, length)?;
        flash_critical(|| self.flash.erase(address, address + length as u32))
            .map_err(|_| Error::IO)?;
        Ok(length)
    }
}

const SHOW_VIRTUAL: u32 = 0x3f700000;

impl DeviceStorage {
    pub fn shows(&mut self) -> Shows<'_> {
        Shows(self)
    }
    pub fn mapped_show(
        &mut self,
        slot: donder_device_storage::show_slots::Slot,
    ) -> Result<&[u8], Error> {
        let offset = slot.data_offset();
        if slot.index >= 2
            || offset
                .checked_add(slot.length)
                .is_none_or(|end| end > donder_device_storage::show_slots::PARTITION_BYTES)
        {
            return Err(Error::INVALID);
        }
        embassy_sync::blocking_mutex::raw::RawMutex::lock(
            &embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex::new(),
            || with_other_core_parked(|| unsafe { flush_show_cache() }),
        );
        // Safety: startup mapped the validated show partition into this unused DROM
        // window. This borrow prevents flash writes through this storage owner while
        // the immutable archive slice is being validated and decoded on core 0.
        Ok(unsafe {
            core::slice::from_raw_parts((SHOW_VIRTUAL as usize + offset) as *const u8, slot.length)
        })
    }
}

pub struct Shows<'a>(&'a mut DeviceStorage);
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
        write_flash(&mut self.0.flash, address, bytes)
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
    if !address.is_multiple_of(4) || !bytes.len().is_multiple_of(4) {
        return Err(Error::INVALID);
    }
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
fn write_flash(_flash: &mut FlashStorage<'_>, address: u32, bytes: &[u8]) -> Result<usize, Error> {
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
    if !address.is_multiple_of(4) || !bytes.len().is_multiple_of(4) {
        return Err(Error::INVALID);
    }
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
    assert!(
        Cpu::current() == Cpu::ProCpu,
        "Flash storage belongs to core 0"
    );
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

fn map_shows(physical: u32) -> Result<(), &'static str> {
    unsafe extern "C" {
        static _rodata_end: u8;
    }
    if (core::ptr::addr_of!(_rodata_end) as usize).next_multiple_of(65536) > SHOW_VIRTUAL as usize {
        return Err("Firmware rodata overlaps the show mapping window");
    }
    // Map once, during startup before Wi-Fi and the second core start. The SDK's
    // flash writer subsequently flushes caches while parking the rendering core.
    let result = embassy_sync::blocking_mutex::raw::RawMutex::lock(
        &embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex::new(),
        || unsafe { map_shows_rom(physical) },
    );
    if result != 0 {
        return Err("Cannot map show partition");
    }
    Ok(())
}

#[esp_hal::ram]
unsafe fn map_shows_rom(physical: u32) -> u32 {
    unsafe extern "C" {
        fn cache_flash_mmu_set_rom(
            cpu: i32,
            pid: i32,
            virtual_address: u32,
            physical_address: u32,
            page_kib: i32,
            pages: i32,
        ) -> u32;
        fn Cache_Read_Disable_rom(cpu: u32);
        fn Cache_Read_Enable_rom(cpu: u32);
        fn Cache_Flush_rom(cpu: u32);
    }
    // Safety: the other core is not running, interrupts are disabled, this code
    // executes from IRAM, and the validated partition/window are 64-KiB aligned.
    // No existing application DROM page is replaced. Only core 0 decodes archives.
    unsafe {
        Cache_Read_Disable_rom(0);
        let result = cache_flash_mmu_set_rom(0, 0, SHOW_VIRTUAL, physical, 64, 4);
        Cache_Flush_rom(0);
        Cache_Read_Enable_rom(0);
        result
    }
}

#[esp_hal::ram]
unsafe fn flush_show_cache() {
    unsafe extern "C" {
        fn Cache_Read_Disable_rom(cpu: u32);
        fn Cache_Read_Enable_rom(cpu: u32);
        fn Cache_Flush_rom(cpu: u32);
    }
    // ESP32 cache/MMU operations can disturb the other core's in-flight cache
    // fill even when it does not read the show. Follow the SDK's both-cache
    // policy: the other core is parked, interrupts are disabled, and all code
    // here executes from IRAM or ROM. Preserve the pre-existing cache enables.
    let dport = esp_hal::peripherals::DPORT::regs();
    let app_enabled = dport.app_cache_ctrl().read().app_cache_enable().bit();
    unsafe {
        Cache_Read_Disable_rom(0);
        if app_enabled {
            Cache_Read_Disable_rom(1);
        }
        Cache_Flush_rom(0);
        if app_enabled {
            Cache_Flush_rom(1);
        }
        Cache_Read_Enable_rom(0);
        if app_enabled {
            Cache_Read_Enable_rom(1);
        }
    }
}
