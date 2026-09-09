use crate::dto::{DeviceFirmwareInfo, DeviceInstallProgress};
use dawn_package::sha256_hex;
use espflash::{
    connection::{Connection, ResetAfterOperation, ResetBeforeOperation},
    flasher::{DeviceInfo, FlashSize, Flasher},
    target::{Chip, ProgressCallbacks, XtalFrequency, efuse::esp32},
};
use std::time::Duration;

const IMAGE: &[u8] = include_bytes!("../../assets/firmware/dawn-esp32.bin");
const IMAGE_HASH: &str = include_str!("../../assets/firmware/dawn-esp32.sha256");
const PARTITIONS: &str = include_str!("../../../../firmware/esp32/partitions.csv");

fn validate_image(image: &[u8], expected_hash: &str) -> Result<(), String> {
    if !sha256_hex(image).eq_ignore_ascii_case(expected_hash.trim()) {
        return Err("Bundled controller image failed its checksum. Rebuild the controller image or reinstall Dawn.".into());
    }
    let table = esp_idf_part::PartitionTable::try_from_str(PARTITIONS)
        .map_err(|error| error.to_string())?;
    table.validate().map_err(|error| error.to_string())?;
    let encoded = table.to_bin().map_err(|error| error.to_string())?;
    let data = table
        .find("dawn")
        .ok_or("Controller image has no Dawn data partition.")?;
    let app = table
        .find("factory")
        .ok_or("Controller image has no application partition.")?;
    if image.get(0x8000..0x8000 + encoded.len()) != Some(encoded.as_slice())
        || image.get(0x1000) != Some(&0xe9)
        || image.get(app.offset() as usize) != Some(&0xe9)
        || image.len().div_ceil(4096) * 4096 > data.offset() as usize
        || data.offset().checked_add(data.size()) != Some(FlashSize::_4Mb.size())
    {
        return Err("Bundled controller image does not match the current flash layout. Rebuild the controller image.".into());
    }
    Ok(())
}

pub(crate) fn info() -> Result<DeviceFirmwareInfo, String> {
    validate_image(IMAGE, IMAGE_HASH)?;
    Ok(DeviceFirmwareInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        image_bytes: IMAGE.len() as u32,
        sha256: IMAGE_HASH.trim().to_ascii_lowercase(),
    })
}

fn validate_device(info: &DeviceInfo) -> Result<(), String> {
    if info.chip != Chip::Esp32
        || info.flash_size != FlashSize::_4Mb
        || info.crystal_frequency != XtalFrequency::_40Mhz
    {
        return Err(format!(
            "This firmware requires an ESP32 with 4 MB flash and a 40 MHz crystal. Detected {} with {} flash and {} crystal. No firmware was written.",
            info.chip, info.flash_size, info.crystal_frequency
        ));
    }
    Ok(())
}

fn preflight(flasher: &mut Flasher) -> Result<(), String> {
    validate_device(&flasher.device_info().map_err(|error| error.to_string())?)?;
    let mut read = |field| {
        Chip::Esp32
            .read_efuse_le::<u32>(flasher.connection(), field)
            .map_err(|error| error.to_string())
    };
    let encrypted = read(esp32::FLASH_CRYPT_CNT)?.count_ones() % 2 != 0;
    if encrypted || read(esp32::ABS_DONE_0)? != 0 || read(esp32::ABS_DONE_1)? != 0 {
        return Err("This installer cannot replace firmware on a controller with flash encryption or secure boot enabled. No firmware was written.".into());
    }
    if read(esp32::DISABLE_APP_CPU)? != 0
        || (read(esp32::CHIP_CPU_FREQ_RATED)? != 0 && read(esp32::CHIP_CPU_FREQ_LOW)? != 0)
    {
        return Err(
            "This firmware requires a dual-core ESP32 rated for 240 MHz. No firmware was written."
                .into(),
        );
    }
    Ok(())
}

pub(crate) fn install(
    port: &str,
    callback: impl FnMut(DeviceInstallProgress),
) -> Result<(), String> {
    validate_image(IMAGE, IMAGE_HASH)?;
    let port_info = serialport::available_ports()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|info| info.port_name == port)
        .ok_or("Selected USB device is no longer available.")?;
    let serialport::SerialPortType::UsbPort(usb) = port_info.port_type else {
        return Err("Select the controller's USB serial device before installing firmware.".into());
    };
    let mut progress = Progress { callback, total: 0 };
    (progress.callback)(DeviceInstallProgress::Connecting);
    let serial = serialport::new(port, 115_200)
        .dtr_on_open(false)
        .timeout(Duration::from_secs(3))
        .open_native()
        .map_err(|error| {
            format!("Could not open {port}: {error}. Close other serial tools and retry.")
        })?;
    let connection = Connection::new(
        serial,
        usb,
        ResetAfterOperation::HardReset,
        ResetBeforeOperation::DefaultReset,
        115_200,
    );
    let mut flasher = match Flasher::try_connect(
        connection,
        true,
        true,
        false,
        Some(Chip::Esp32),
        None,
    ) {
        Ok(flasher) => flasher,
        Err(error) => {
            let (error, mut connection) = *error;
            let reset = connection.reset();
            return Err(format!(
                "Could not connect to the controller bootloader: {error}. No firmware was written.{}",
                reset
                    .err()
                    .map(|error| format!(" Reset also failed: {error}."))
                    .unwrap_or_default()
            ));
        }
    };
    if let Err(error) = preflight(&mut flasher) {
        return Err(match flasher.connection().reset() {
            Ok(()) => error,
            Err(reset) => format!("{error} Reset also failed: {reset}."),
        });
    }
    // Match the reliable transfer rate used by the controller acceptance runs.
    if let Err(error) = flasher.change_baud(19_200) {
        let message =
            format!("Could not set installation speed: {error}. No firmware was written.");
        return Err(match flasher.connection().reset() {
            Ok(()) => message,
            Err(reset) => format!("{message} Reset also failed: {reset}."),
        });
    }
    flasher.write_bin_to_flash(0, IMAGE, &mut progress)
        .map_err(|error| format!("Firmware installation did not complete: {error}. The controller may contain an incomplete image. Reconnect and retry installation."))
}

struct Progress<F> {
    callback: F,
    total: u32,
}

impl<F: FnMut(DeviceInstallProgress)> ProgressCallbacks for Progress<F> {
    fn init(&mut self, _: u32, total: usize) {
        self.total = total as u32;
        (self.callback)(DeviceInstallProgress::Writing {
            completed: 0,
            total: self.total,
        });
    }
    fn update(&mut self, completed: usize) {
        (self.callback)(DeviceInstallProgress::Writing {
            completed: completed as u32,
            total: self.total,
        });
    }
    fn verifying(&mut self) {
        (self.callback)(DeviceInstallProgress::Verifying);
    }
    fn finish(&mut self, _: bool) {
        (self.callback)(DeviceInstallProgress::Restarting);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_image_matches_checksum_and_current_partition_layout() {
        validate_image(IMAGE, IMAGE_HASH).unwrap();
        let mut damaged = IMAGE.to_vec();
        damaged[0x10000] ^= 1;
        assert!(
            validate_image(&damaged, IMAGE_HASH)
                .unwrap_err()
                .contains("checksum")
        );
        damaged = IMAGE.to_vec();
        damaged[0x8000] ^= 1;
        let hash = sha256_hex(&damaged);
        assert!(
            validate_image(&damaged, &hash)
                .unwrap_err()
                .contains("flash layout")
        );
    }

    #[test]
    fn incompatible_chip_flash_or_crystal_is_rejected() {
        let mut device = DeviceInfo {
            chip: Chip::Esp32,
            revision: Some((3, 1)),
            crystal_frequency: XtalFrequency::_40Mhz,
            flash_size: FlashSize::_4Mb,
            features: Vec::new(),
            mac_address: None,
        };
        validate_device(&device).unwrap();
        device.chip = Chip::Esp32s3;
        assert!(validate_device(&device).is_err());
        device.chip = Chip::Esp32;
        device.flash_size = FlashSize::_2Mb;
        assert!(validate_device(&device).is_err());
        device.flash_size = FlashSize::_4Mb;
        device.crystal_frequency = XtalFrequency::_26Mhz;
        assert!(validate_device(&device).is_err());
    }
}
