use super::*;
use alloc::vec;

fn stage_show(flash: &mut Flash, bytes: &[u8]) -> Result<show_slots::Slot, Error> {
    let slot = show_slots::begin(flash, bytes.len())?;
    for (index, chunk) in bytes.chunks(1024).enumerate() {
        let mut aligned = [0xff; 1024];
        aligned[..chunk.len()].copy_from_slice(chunk);
        show_slots::append(
            flash,
            slot,
            index * 1024,
            &aligned[..chunk.len().next_multiple_of(4)],
        )?;
    }
    Ok(slot)
}

#[test]
fn staged_show_does_not_replace_the_committed_show() {
    let mut flash = Flash::blank();
    let first = stage_show(&mut flash, &vec![0x35; 78392]).unwrap();
    show_slots::commit(&mut flash, first).unwrap();
    let second = stage_show(&mut flash, &vec![0x72; 90003]).unwrap();
    assert_eq!(show_slots::latest(&mut flash).unwrap(), Some(first));
    show_slots::commit(&mut flash, second).unwrap();
    assert_eq!(show_slots::latest(&mut flash).unwrap(), Some(second));
    assert_eq!(
        &flash.bytes[second.data_offset()..second.data_offset() + second.length],
        &vec![0x72; 90003]
    );
}

#[test]
fn interrupted_show_replacement_preserves_a_complete_committed_slot() {
    let mut saved = Flash::blank();
    let first = stage_show(&mut saved, &vec![0x35; 78392]).unwrap();
    show_slots::commit(&mut saved, first).unwrap();
    saved.writes = 0;
    let replacement = vec![0x72; 90003];
    let mut complete = saved.clone();
    let second = stage_show(&mut complete, &replacement).unwrap();
    show_slots::commit(&mut complete, second).unwrap();
    for failure in 1..=complete.writes {
        let mut flash = saved.clone();
        flash.fail_at = Some(failure);
        if let Ok(slot) = stage_show(&mut flash, &replacement) {
            let _ = show_slots::commit(&mut flash, slot);
        }
        flash.fail_at = None;
        let selected = show_slots::latest(&mut flash).unwrap().unwrap();
        assert!(selected == first || selected == second);
        let expected = if selected == first { 0x35 } else { 0x72 };
        assert!(
            flash.bytes[selected.data_offset()..selected.data_offset() + selected.length]
                .iter()
                .all(|&byte| byte == expected)
        );
    }
}

#[test]
fn committed_show_corruption_and_invalid_chunk_bounds_are_rejected() {
    let mut flash = Flash::blank();
    let slot = stage_show(&mut flash, &vec![0x35; 78392]).unwrap();
    assert_eq!(
        show_slots::append(&mut flash, slot, slot.length, &[0; 4]),
        Err(Error::INVALID)
    );
    show_slots::commit(&mut flash, slot).unwrap();
    flash.bytes[slot.data_offset() + 1234] ^= 1;
    assert_eq!(show_slots::latest(&mut flash), Err(Error::CORRUPTION));
}

#[derive(Clone)]
struct Flash {
    bytes: Vec<u8>,
    writes: usize,
    fail_at: Option<usize>,
}

impl Flash {
    fn blank() -> Self {
        Self {
            bytes: vec![0xff; 256 * 1024],
            writes: 0,
            fail_at: None,
        }
    }

    fn interrupted(&mut self) -> bool {
        self.writes += 1;
        self.fail_at.is_some_and(|limit| self.writes >= limit)
    }
}

impl Storage for Flash {
    const READ_SIZE: usize = 4;
    const WRITE_SIZE: usize = 4;
    const BLOCK_SIZE: usize = 4096;
    const BLOCK_COUNT: usize = 64;
    const BLOCK_CYCLES: isize = 500;
    type CACHE_SIZE = consts::U256;
    type LOOKAHEAD_SIZE = consts::U1;

    fn read(&mut self, off: usize, buf: &mut [u8]) -> Result<usize, Error> {
        buf.copy_from_slice(&self.bytes[off..off + buf.len()]);
        Ok(buf.len())
    }

    fn write(&mut self, off: usize, data: &[u8]) -> Result<usize, Error> {
        let interrupted = self.interrupted();
        let length = if interrupted {
            data.len() / 2
        } else {
            data.len()
        };
        for (target, &source) in self.bytes[off..off + length].iter_mut().zip(data) {
            assert_eq!(*target & source, source);
            *target = source;
        }
        if interrupted {
            Err(Error::IO)
        } else {
            Ok(data.len())
        }
    }

    fn erase(&mut self, off: usize, len: usize) -> Result<usize, Error> {
        let interrupted = self.interrupted();
        let length = if interrupted { len / 2 } else { len };
        self.bytes[off..off + length].fill(0xff);
        if interrupted { Err(Error::IO) } else { Ok(len) }
    }
}

#[test]
fn records_survive_remount_and_oversized_reads_are_rejected() {
    let mut flash = Flash::blank();
    initialize(&mut flash).unwrap();
    assert!(read(&mut flash, Record::Device, 100).unwrap().is_none());
    let record = vec![0x42; 1024];
    write(&mut flash, Record::Device, &record).unwrap();
    initialize(&mut flash).unwrap();
    assert_eq!(
        read(&mut flash, Record::Device, record.len())
            .unwrap()
            .unwrap(),
        record
    );
    assert_eq!(
        read(&mut flash, Record::Device, 100).unwrap_err(),
        Error::FILE_TOO_BIG
    );
}

#[test]
fn torn_replacement_keeps_a_complete_old_or_new_record() {
    let mut saved = Flash::blank();
    initialize(&mut saved).unwrap();
    let old = vec![0x35; 1024];
    let new = vec![0x72; 1024];
    write(&mut saved, Record::Device, &old).unwrap();
    saved.writes = 0;
    let mut complete = saved.clone();
    write(&mut complete, Record::Device, &new).unwrap();
    for failure in 1..=complete.writes {
        let mut flash = saved.clone();
        flash.fail_at = Some(failure);
        let _ = write(&mut flash, Record::Device, &new);
        flash.fail_at = None;
        initialize(&mut flash).unwrap();
        let recovered = read(&mut flash, Record::Device, new.len())
            .unwrap()
            .unwrap();
        assert!(recovered == old || recovered == new, "failure at {failure}");
    }
}

#[test]
fn damaged_partition_is_not_automatically_formatted() {
    let mut flash = Flash::blank();
    flash.bytes[8000] = 0;
    let before = flash.bytes.clone();
    assert!(initialize(&mut flash).is_err());
    assert_eq!(flash.bytes, before);
}

#[test]
fn device_config_roundtrip_and_invalid_config_cannot_replace_saved_values() {
    use device_config::{DeviceConfig, Network};
    let mut flash = Flash::blank();
    initialize(&mut flash).unwrap();
    assert!(DeviceConfig::load(&mut flash).unwrap().is_none());
    let mut config = DeviceConfig {
        name: "Porch \u{e9}".into(),
        network: Some(Network {
            ssid: "caf\u{e9}".into(),
            password: "long-\"password\\with\nescapes".into(),
        }),
        token: Some([b'a'; 32]),
    };
    config.save(&mut flash).unwrap();
    let loaded = DeviceConfig::load(&mut flash).unwrap().unwrap();
    assert_eq!(loaded.name, config.name);
    let network = loaded.network.unwrap();
    assert_eq!(network.ssid, "caf\u{e9}");
    assert_eq!(network.password, "long-\"password\\with\nescapes");
    assert_eq!(loaded.token, config.token);
    config.token = Some([b'!'; 32]);
    assert!(config.save(&mut flash).is_err());
    config.token = None;
    config.name = "\n".into();
    assert!(config.save(&mut flash).is_err());
    assert_eq!(
        DeviceConfig::load(&mut flash).unwrap().unwrap().token,
        Some([b'a'; 32])
    );
    let unclaimed = DeviceConfig {
        name: "Donder-3f2a".into(),
        network: None,
        token: None,
    };
    unclaimed.save(&mut flash).unwrap();
    let loaded = DeviceConfig::load(&mut flash).unwrap().unwrap();
    assert!(loaded.network.is_none() && loaded.token.is_none());
    assert_eq!(
        Network::from_json(br#"{"ssid":"home","password":"12345678"}"#)
            .unwrap()
            .ssid,
        "home"
    );
    assert!(Network::from_json(br#"{"ssid":"home","password":"short"}"#).is_err());
}

#[test]
fn corrupted_record_payload_is_rejected() {
    let mut flash = Flash::blank();
    initialize(&mut flash).unwrap();
    let bytes = vec![0x42; 1024];
    write(&mut flash, Record::Device, &bytes).unwrap();
    let offset = flash
        .bytes
        .windows(256)
        .position(|chunk| chunk.iter().all(|&byte| byte == 0x42))
        .unwrap();
    flash.bytes[offset] ^= 1;
    assert_eq!(
        read(&mut flash, Record::Device, bytes.len()).unwrap_err(),
        Error::CORRUPTION
    );
}

#[test]
fn explicit_erase_recovers_corruption_and_removes_all_saved_records() {
    let mut flash = Flash::blank();
    initialize(&mut flash).unwrap();
    write(&mut flash, Record::Device, b"device").unwrap();
    flash.bytes[..4096].fill(0);
    flash.bytes[4096..8192].fill(0);
    assert!(initialize(&mut flash).is_err());
    erase_all(&mut flash).unwrap();
    assert!(flash.bytes.iter().all(|&byte| byte == 0xff));
    initialize(&mut flash).unwrap();
    assert!(read(&mut flash, Record::Device, 100).unwrap().is_none());
}
