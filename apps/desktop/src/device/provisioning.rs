use crate::dto::{DeviceSerialPort, ProvisionedDevice};
use std::{
    io::{ErrorKind, Read, Write},
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};

pub(crate) fn ports() -> Result<Vec<DeviceSerialPort>, String> {
    let mut ports = serialport::available_ports()
        .map_err(|error| format!("Could not list USB serial devices: {error}"))?
        .into_iter()
        .map(|port| {
            let label = match port.port_type {
                serialport::SerialPortType::UsbPort(usb) => format!(
                    "{}: {}",
                    port.port_name,
                    usb.product
                        .unwrap_or_else(|| format!("USB {:04x}:{:04x}", usb.vid, usb.pid))
                ),
                _ => port.port_name.clone(),
            };
            DeviceSerialPort {
                path: port.port_name,
                label,
            }
        })
        .collect::<Vec<_>>();
    ports.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(ports)
}

pub(crate) fn provision(
    path: &str,
    ssid: &str,
    password: &str,
) -> Result<ProvisionedDevice, String> {
    validate_credentials(ssid, password)?;
    let mut port = open_reset_port(path)?;
    exchange(&mut *port, ssid, password)
}

pub(crate) fn erase_saved_data(path: &str) -> Result<(), String> {
    let mut port = open_reset_port(path)?;
    erase_exchange(&mut *port)
}

fn open_reset_port(path: &str) -> Result<Box<dyn serialport::SerialPort>, String> {
    let mut port = serialport::new(path, 115_200)
        .dtr_on_open(false)
        .timeout(Duration::from_millis(100))
        .open()
        .map_err(|error| {
            format!("Could not open {path}: {error}. Close other serial monitors and try again.")
        })?;
    port.write_data_terminal_ready(false)
        .map_err(|error| error.to_string())?;
    port.write_request_to_send(true)
        .map_err(|error| error.to_string())?;
    std::thread::sleep(Duration::from_millis(100));
    let cleared = port.clear(serialport::ClearBuffer::Input);
    port.write_request_to_send(false)
        .map_err(|error| error.to_string())?;
    cleared.map_err(|error| error.to_string())?;
    Ok(port)
}

fn validate_credentials(ssid: &str, password: &str) -> Result<(), String> {
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("Wi-Fi network name must contain 1-32 UTF-8 bytes.".into());
    }
    if !(8..=64).contains(&password.len()) {
        return Err("Enter an 8-64 byte WPA2 personal network password.".into());
    }
    Ok(())
}

// Preserve partial lines across serial timeouts; boot logs may be fragmented.
fn read_line(
    port: &mut (impl Read + ?Sized),
    pending: &mut Vec<u8>,
    deadline: Instant,
) -> Result<Option<Vec<u8>>, String> {
    let line = read_line_raw(port, pending, deadline)?;
    if let Some(error) = line
        .as_deref()
        .and_then(|line| line.strip_prefix(b"DAWN ERROR "))
    {
        return Err(format!("Controller: {}", String::from_utf8_lossy(error)));
    }
    Ok(line)
}

fn read_line_raw(
    port: &mut (impl Read + ?Sized),
    pending: &mut Vec<u8>,
    deadline: Instant,
) -> Result<Option<Vec<u8>>, String> {
    loop {
        if Instant::now() >= deadline {
            return Ok(None);
        }
        let mut byte = [0];
        match port.read_exact(&mut byte) {
            Ok(()) => {
                if byte[0] == b'\n' {
                    return Ok(Some(std::mem::take(pending)));
                }
                if pending.len() == 1024 {
                    return Err("Device serial response exceeded the expected line length.".into());
                }
                pending.push(byte[0]);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(format!("USB connection failed: {error}")),
        }
    }
}

fn erase_exchange(port: &mut (impl Read + Write + ?Sized)) -> Result<(), String> {
    let mut pending = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if Instant::now() >= deadline {
            return Err("Controller did not enter storage recovery. Check the selected port and installed firmware.".into());
        }
        port.write_all(b"R")
            .map_err(|error| format!("USB recovery handshake failed: {error}"))?;
        let Some(line) = read_line_raw(port, &mut pending, deadline)? else {
            continue;
        };
        if line == b"DAWN RESET READY" {
            break;
        }
        if let Some(error) = line.strip_prefix(b"DAWN RESET UNAVAILABLE ") {
            return Err(format!(
                "Controller cannot erase saved data: {}",
                String::from_utf8_lossy(error)
            ));
        }
    }
    port.write_all(b"F")
        .map_err(|error| format!("Could not confirm the storage erase request: {error}. Saved data may have been erased; reconnect before retrying."))?;
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if Instant::now() >= deadline {
            return Err("Storage erase was not acknowledged. Saved data may have been erased; reconnect before retrying.".into());
        }
        if read_line(port, &mut pending, deadline)
            .map_err(|error| {
                format!("Erase completion is unknown: {error}. Reconnect before retrying.")
            })?
            .as_deref()
            == Some(b"DAWN RESET COMPLETE")
        {
            return Ok(());
        }
    }
}

fn exchange(
    port: &mut (impl Read + Write + ?Sized),
    ssid: &str,
    password: &str,
) -> Result<ProvisionedDevice, String> {
    let mut pending = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if Instant::now() >= deadline {
            return Err("Dawn firmware did not answer. Check the selected USB port and install the loader firmware.".into());
        }
        port.write_all(b"P")
            .map_err(|error| format!("USB handshake failed: {error}"))?;
        if read_line(port, &mut pending, deadline)?.as_deref() == Some(b"DAWN PROVISION READY") {
            break;
        }
    }
    let mut credentials = Vec::with_capacity(3 + ssid.len() + password.len());
    credentials.extend_from_slice(&[b'W', ssid.len() as u8, password.len() as u8]);
    credentials.extend_from_slice(ssid.as_bytes());
    credentials.extend_from_slice(password.as_bytes());
    let written = port.write_all(&credentials);
    credentials.fill(0);
    written.map_err(|error| format!("Could not send Wi-Fi credentials: {error}"))?;
    let token_deadline = Instant::now() + Duration::from_secs(5);
    let token = loop {
        if Instant::now() >= token_deadline {
            return Err("Device did not return a connection token after provisioning.".into());
        }
        if let Some(line) = read_line(port, &mut pending, token_deadline)?
            && let Some(token) = line.strip_prefix(b"TOKEN ")
        {
            if token.len() != 32 || !token.iter().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("Device returned an invalid connection token.".into());
            }
            break String::from_utf8(token.to_vec()).map_err(|_| "Invalid token encoding.")?;
        }
    };
    let wifi_deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if Instant::now() >= wifi_deadline {
            return Err("Device could not join Wi-Fi. Check the network name, password, and 2.4 GHz network availability, then provision again.".into());
        }
        let Some(line) = read_line(port, &mut pending, wifi_deadline)? else {
            continue;
        };
        let Some(marker) = line
            .windows(b"WIFI READY ".len())
            .position(|part| part == b"WIFI READY ")
        else {
            continue;
        };
        let text = std::str::from_utf8(&line[marker + b"WIFI READY ".len()..])
            .map_err(|_| "Invalid Wi-Fi response encoding.")?;
        let mut fields = text.split_whitespace();
        let ip = fields
            .next()
            .ok_or("Device did not return an IP address.")?
            .parse::<IpAddr>()
            .map_err(|_| "Device returned an invalid IP address.")?;
        let port = fields
            .next()
            .ok_or("Device did not return an HTTP port.")?
            .parse::<u16>()
            .map_err(|_| "Device returned an invalid HTTP port.")?;
        if port == 0 || ip.is_unspecified() {
            return Err("Device returned an unusable network address.".into());
        }
        return Ok(ProvisionedDevice {
            address: SocketAddr::new(ip, port).to_string(),
            token,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct SerialTranscript {
        input: VecDeque<Option<u8>>,
        written: Vec<u8>,
    }
    impl Read for SerialTranscript {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            match self.input.pop_front() {
                Some(Some(byte)) => {
                    buffer[0] = byte;
                    Ok(1)
                }
                Some(None) => Err(ErrorKind::TimedOut.into()),
                None => Err(ErrorKind::UnexpectedEof.into()),
            }
        }
    }
    impl Write for SerialTranscript {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.written.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn device_storage_errors_are_reported_during_provisioning() {
        let mut port = SerialTranscript {
            input: b"DAWN ERROR Cannot mount Dawn storage; data was not erased\n"
                .iter()
                .copied()
                .map(Some)
                .collect(),
            written: Vec::new(),
        };
        let error = exchange(&mut port, "network", "password").err().unwrap();
        assert!(error.contains("Cannot mount Dawn storage"));
        assert!(!port.written.contains(&b'W'));
    }

    #[test]
    fn erase_requires_ready_and_accepts_recovery_from_damaged_storage() {
        let mut port = SerialTranscript {
            input: b"DAWN ERROR Cannot mount storage\nDAWN RESET READY\nDAWN RESET COMPLETE\n"
                .iter()
                .copied()
                .map(Some)
                .collect(),
            written: Vec::new(),
        };
        erase_exchange(&mut port).unwrap();
        assert_eq!(port.written, b"RRF");
        let mut rejected = SerialTranscript {
            input: b"DAWN RESET UNAVAILABLE Invalid partition table\n"
                .iter()
                .copied()
                .map(Some)
                .collect(),
            written: Vec::new(),
        };
        assert!(
            erase_exchange(&mut rejected)
                .unwrap_err()
                .contains("Invalid partition table")
        );
        assert_eq!(rejected.written, b"R");
    }

    #[test]
    fn erase_failure_is_not_reported_as_success() {
        let mut port = SerialTranscript {
            input: b"DAWN RESET READY\nDAWN ERROR Storage erase failed\n"
                .iter()
                .copied()
                .map(Some)
                .collect(),
            written: Vec::new(),
        };
        assert!(
            erase_exchange(&mut port)
                .unwrap_err()
                .contains("Storage erase failed")
        );
        assert_eq!(port.written, b"RF");
    }
    #[test]
    fn provisioning_preserves_fragmented_replies_and_uses_utf8_byte_lengths() {
        let mut input = VecDeque::new();
        for part in [
            b"boot log\nDAWN PRO".as_slice(),
            b"VISION READY\nTOKEN aaaaaaaaaaaa",
            b"aaaaaaaaaaaaaaaaaaaa\nWIFI CONNECTED\n",
            b"log: WIFI READY 192.168.1.50 80\n",
        ] {
            input.extend(part.iter().copied().map(Some));
            input.push_back(None);
        }
        let mut serial = SerialTranscript {
            input,
            written: Vec::new(),
        };
        let result = exchange(&mut serial, "caf\u{e9}", "password").unwrap();
        assert_eq!(result.address, "192.168.1.50:80");
        assert_eq!(result.token, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let credentials = serial
            .written
            .iter()
            .position(|byte| *byte == b'W')
            .unwrap();
        assert!(
            serial.written[..credentials]
                .iter()
                .all(|byte| *byte == b'P')
        );
        assert_eq!(
            &serial.written[credentials..],
            b"W\x05\x08caf\xc3\xa9password"
        );
    }
    #[test]
    fn provisioning_rejects_malformed_tokens_without_echoing_serial_contents() {
        let transcript = b"DAWN PROVISION READY\nTOKEN secret-invalid-token\n";
        let mut serial = SerialTranscript {
            input: transcript.iter().copied().map(Some).collect(),
            written: Vec::new(),
        };
        let error = exchange(&mut serial, "network", "password").err().unwrap();
        assert!(error.contains("invalid connection token"));
        assert!(!error.contains("secret-invalid-token"));
        assert!(validate_credentials(&"a".repeat(33), "password").is_err());
        assert!(validate_credentials("network", "short").is_err());
    }
}
