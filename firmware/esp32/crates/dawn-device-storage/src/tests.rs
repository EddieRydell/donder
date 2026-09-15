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
fn records_survive_remount_and_oversized_reads_are_rejected() {
    let mut flash = Flash::blank();
    initialize(&mut flash).unwrap();
    assert!(read(&mut flash, Record::Sequence, 100).unwrap().is_none());
    write(&mut flash, Record::Credentials, b"credentials").unwrap();
    let sequence = vec![0x42; 32 * 1024 + 16];
    write(&mut flash, Record::Sequence, &sequence).unwrap();
    initialize(&mut flash).unwrap();
    assert_eq!(
        read(&mut flash, Record::Sequence, sequence.len())
            .unwrap()
            .unwrap(),
        sequence
    );
    assert_eq!(
        read(&mut flash, Record::Sequence, 100).unwrap_err(),
        Error::FILE_TOO_BIG
    );
    assert_eq!(
        read(&mut flash, Record::Credentials, 100).unwrap().unwrap(),
        b"credentials"
    );
}

#[test]
fn torn_replacement_keeps_a_complete_old_or_new_record() {
    let mut saved = Flash::blank();
    initialize(&mut saved).unwrap();
    let old = vec![0x35; 32 * 1024 + 16];
    let new = vec![0x72; 32 * 1024 + 16];
    write(&mut saved, Record::Sequence, &old).unwrap();
    write(&mut saved, Record::Credentials, b"credentials").unwrap();
    saved.writes = 0;
    let mut complete = saved.clone();
    write(&mut complete, Record::Sequence, &new).unwrap();
    for failure in 1..=complete.writes {
        let mut flash = saved.clone();
        flash.fail_at = Some(failure);
        let _ = write(&mut flash, Record::Sequence, &new);
        flash.fail_at = None;
        initialize(&mut flash).unwrap();
        let recovered = read(&mut flash, Record::Sequence, new.len())
            .unwrap()
            .unwrap();
        assert!(recovered == old || recovered == new, "failure at {failure}");
        assert_eq!(
            read(&mut flash, Record::Credentials, 100).unwrap().unwrap(),
            b"credentials"
        );
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
fn credentials_roundtrip_and_invalid_credentials_cannot_replace_saved_values() {
    let mut flash = Flash::blank();
    initialize(&mut flash).unwrap();
    let mut credentials = credentials::Credentials {
        ssid: "caf\u{e9}".into(),
        password: "long-\"password\\with\nescapes".into(),
        token: [b'a'; 32],
    };
    credentials.save(&mut flash).unwrap();
    let loaded = credentials::Credentials::load(&mut flash).unwrap().unwrap();
    assert_eq!(loaded.ssid, credentials.ssid);
    assert_eq!(loaded.password, credentials.password);
    assert_eq!(loaded.token, credentials.token);
    credentials.token[0] = b'!';
    assert!(credentials.save(&mut flash).is_err());
    assert_eq!(
        credentials::Credentials::load(&mut flash)
            .unwrap()
            .unwrap()
            .token,
        [b'a'; 32]
    );
}

#[test]
fn corrupted_record_payload_is_rejected() {
    let mut flash = Flash::blank();
    initialize(&mut flash).unwrap();
    let bytes = vec![0x42; 32 * 1024];
    write(&mut flash, Record::Sequence, &bytes).unwrap();
    let offset = flash
        .bytes
        .windows(256)
        .position(|chunk| chunk.iter().all(|&byte| byte == 0x42))
        .unwrap();
    flash.bytes[offset] ^= 1;
    assert_eq!(
        read(&mut flash, Record::Sequence, bytes.len()).unwrap_err(),
        Error::CORRUPTION
    );
}

#[test]
fn explicit_erase_recovers_corruption_and_removes_all_saved_records() {
    let mut flash = Flash::blank();
    initialize(&mut flash).unwrap();
    write(&mut flash, Record::Credentials, b"credentials").unwrap();
    write(&mut flash, Record::Sequence, b"sequence").unwrap();
    flash.bytes[..4096].fill(0);
    flash.bytes[4096..8192].fill(0);
    assert!(initialize(&mut flash).is_err());
    erase_all(&mut flash).unwrap();
    assert!(flash.bytes.iter().all(|&byte| byte == 0xff));
    initialize(&mut flash).unwrap();
    assert!(
        read(&mut flash, Record::Credentials, 100)
            .unwrap()
            .is_none()
    );
    assert!(read(&mut flash, Record::Sequence, 100).unwrap().is_none());
}
