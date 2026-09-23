use donder_device_storage::{Error, Storage, consts};
use esp_bootloader_esp_idf::partitions::{PARTITION_TABLE_MAX_LEN, read_partition_table};
use esp_storage::{Flash, FlashStorage};

pub struct DeviceStorage {
    flash: FlashStorage<'static>,
    offset: u32,
}

// Avoid esp-storage's sector-sized stack scratch space for unaligned C buffers.
#[repr(align(4))]
struct AlignedBytes([u8; 256]);

impl DeviceStorage {
    pub fn new(peripheral: Flash<'static>) -> Result<Self, &'static str> {
        let mut flash = FlashStorage::new(peripheral).multicore_auto_park();
        let mut bytes = [0; PARTITION_TABLE_MAX_LEN];
        let table =
            read_partition_table(&mut flash, &mut bytes).map_err(|_| "Invalid partition table")?;
        let mut matches = table.iter().filter(|entry| entry.label_as_str() == "donder");
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
        Ok(Self {
            flash,
            offset: entry.offset(),
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
        let address = self.address(offset, bytes.len())?;
        let mut scratch = AlignedBytes([0; 256]);
        for (index, chunk) in bytes.chunks_mut(256).enumerate() {
            self.flash
                .read_nor(
                    address + (index * 256) as u32,
                    &mut scratch.0[..chunk.len()],
                )
                .map_err(|_| Error::IO)?;
            chunk.copy_from_slice(&scratch.0[..chunk.len()]);
        }
        Ok(bytes.len())
    }

    fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<usize, Error> {
        let address = self.address(offset, bytes.len())?;
        let mut scratch = AlignedBytes([0; 256]);
        for (index, chunk) in bytes.chunks(256).enumerate() {
            scratch.0[..chunk.len()].copy_from_slice(chunk);
            self.flash
                .write_nor(address + (index * 256) as u32, &scratch.0[..chunk.len()])
                .map_err(|_| Error::IO)?;
        }
        Ok(bytes.len())
    }

    fn erase(&mut self, offset: usize, length: usize) -> Result<usize, Error> {
        let address = self.address(offset, length)?;
        self.flash
            .erase(address, address + length as u32)
            .map_err(|_| Error::IO)?;
        Ok(length)
    }
}
