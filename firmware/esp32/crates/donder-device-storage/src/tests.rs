use super::*;
use alloc::vec;

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
fn records_survive_remount() {
    let mut flash = Flash::blank();
    initialize(&mut flash).unwrap();
    assert!(read(&mut flash).unwrap().is_none());
    let record = vec![0x42; 1024];
    write(&mut flash, &record).unwrap();
    initialize(&mut flash).unwrap();
    assert_eq!(read(&mut flash).unwrap().unwrap(), record);
}

#[test]
fn torn_replacement_keeps_a_complete_old_or_new_record() {
    let mut saved = Flash::blank();
    initialize(&mut saved).unwrap();
    let old = vec![0x35; 1024];
    let new = vec![0x72; 1024];
    write(&mut saved, &old).unwrap();
    saved.writes = 0;
    let mut complete = saved.clone();
    write(&mut complete, &new).unwrap();
    for failure in 1..=complete.writes {
        let mut flash = saved.clone();
        flash.fail_at = Some(failure);
        let _ = write(&mut flash, &new);
        flash.fail_at = None;
        initialize(&mut flash).unwrap();
        let recovered = read(&mut flash).unwrap().unwrap();
        assert!(recovered == old || recovered == new, "failure at {failure}");
    }
}

#[test]
fn damaged_partition_is_formatted() {
    let mut flash = Flash::blank();
    flash.bytes[8000] = 0;
    initialize(&mut flash).unwrap();
    assert!(read(&mut flash).unwrap().is_none());
}

#[test]
fn device_config_roundtrip() {
    use device_config::{DeviceConfig, network_from_json};
    use donder_device_protocol::NetworkCredentials as Network;
    let mut flash = Flash::blank();
    initialize(&mut flash).unwrap();
    assert!(DeviceConfig::load(&mut flash).unwrap().is_none());
    let config = DeviceConfig {
        name: "Porch \u{e9}".into(),
        network: Some(Network {
            ssid: "caf\u{e9}".into(),
            password: "long-\"password\\with\nescapes".into(),
        }),
    };
    config.save(&mut flash).unwrap();
    let loaded = DeviceConfig::load(&mut flash).unwrap().unwrap();
    assert_eq!(loaded.name, config.name);
    let network = loaded.network.unwrap();
    assert_eq!(network.ssid, "caf\u{e9}");
    assert_eq!(network.password, "long-\"password\\with\nescapes");
    let fresh = DeviceConfig {
        name: "Donder-3f2a".into(),
        network: None,
    };
    fresh.save(&mut flash).unwrap();
    let loaded = DeviceConfig::load(&mut flash).unwrap().unwrap();
    assert!(loaded.network.is_none());
    assert_eq!(
        network_from_json(br#"{"ssid":"home","password":"12345678"}"#)
            .unwrap()
            .ssid,
        "home"
    );
    assert!(network_from_json(br#"{"ssid":"home","password":"short"}"#).is_err());
}
