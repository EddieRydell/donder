use crate::dto::DeviceSerialPort;
use std::{
    io::{ErrorKind, Read, Write},
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

// Preserve partial lines across serial timeouts; boot logs may be fragmented.
fn read_line(
    port: &mut (impl Read + ?Sized),
    pending: &mut Vec<u8>,
    deadline: Instant,
) -> Result<Option<Vec<u8>>, String> {
    let line = read_line_raw(port, pending, deadline)?;
    if let Some(error) = line
        .as_deref()
        .and_then(|line| line.strip_prefix(b"DONDER ERROR "))
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
        if line == b"DONDER RESET READY" {
            break;
        }
        if let Some(error) = line.strip_prefix(b"DONDER RESET UNAVAILABLE ") {
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
            == Some(b"DONDER RESET COMPLETE")
        {
            return Ok(());
        }
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
    fn erase_requires_ready_and_accepts_recovery_from_damaged_storage() {
        let mut port = SerialTranscript {
            input:
                b"DONDER ERROR Cannot mount storage\nDONDER RESET READY\nDONDER RESET COMPLETE\n"
                    .iter()
                    .copied()
                    .map(Some)
                    .collect(),
            written: Vec::new(),
        };
        erase_exchange(&mut port).unwrap();
        assert_eq!(port.written, b"RRF");
        let mut rejected = SerialTranscript {
            input: b"DONDER RESET UNAVAILABLE Invalid partition table\n"
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
            input: b"DONDER RESET READY\nDONDER ERROR Storage erase failed\n"
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
}
