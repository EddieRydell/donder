pub(crate) mod firmware;
pub(crate) mod provisioning;

use crate::dto::{
    DeviceCapabilities, DeviceOutputCapabilities, DevicePlaybackMode, DeviceTransportStatus,
};
use reqwest::{
    blocking::Client,
    header::{HeaderMap, HeaderValue},
};
use std::{io::Read, net::SocketAddr, time::Duration};

pub(crate) struct DeviceClient {
    client: Client,
    address: SocketAddr,
}

impl DeviceClient {
    pub(crate) fn new(address: &str, token: &str) -> Result<Self, String> {
        let address = address
            .parse::<SocketAddr>()
            .map_err(|_| "Enter the device IP address and port, for example 192.168.1.50:80.")?;
        if token.len() != 32 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Enter the 32-character token returned during device provisioning.".into());
        }
        let mut value = HeaderValue::from_str(token).map_err(|_| "Invalid device token.")?;
        value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert("x-dawn-token", value);
        let client = Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| format!("Could not initialize device connection: {error}"))?;
        Ok(Self { client, address })
    }

    fn response(&self, response: reqwest::blocking::Response) -> Result<String, String> {
        let status = response.status();
        let mut bytes = Vec::new();
        response
            .take(8193)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Could not read device response: {error}"))?;
        if bytes.len() > 8192 {
            return Err("Device response exceeded its expected size.".into());
        }
        let text =
            String::from_utf8(bytes).map_err(|_| "Device returned invalid response text.")?;
        if !status.is_success() {
            return Err(format!("Device returned HTTP {status}: {}", text.trim()));
        }
        Ok(text)
    }

    pub(crate) fn capabilities(&self) -> Result<DeviceCapabilities, String> {
        let response = self
            .client
            .get(format!("http://{}/capabilities", self.address))
            .send()
            .map_err(|error| format!("Could not contact device: {error}"))?;
        serde_json::from_str(&self.response(response)?)
            .map_err(|error| format!("Device returned invalid capabilities: {error}"))
    }

    pub(crate) fn transport(
        &self,
        mode: Option<DevicePlaybackMode>,
    ) -> Result<DeviceTransportStatus, String> {
        let request = match mode {
            None => self
                .client
                .get(format!("http://{}/transport", self.address)),
            Some(mode) => {
                let action = match mode {
                    DevicePlaybackMode::Playing => "play",
                    DevicePlaybackMode::Paused => "pause",
                    DevicePlaybackMode::Stopped => "stop",
                };
                self.client
                    .post(format!("http://{}/transport/{action}", self.address))
                    .body(Vec::new())
            }
        };
        let response = request.send().map_err(|error| format!("Device transport request did not finish: {error}. Refresh device status before retrying."))?;
        serde_json::from_str(&self.response(response)?)
            .map_err(|error| format!("Device returned invalid playback status: {error}"))
    }

    pub(crate) fn upload(&self, bytes: Vec<u8>, widths: &[u16]) -> Result<String, String> {
        let capabilities = self.capabilities()?;
        let header: &[u8; 16] = bytes
            .get(..16)
            .and_then(|header| header.try_into().ok())
            .ok_or("Compiled sequence header is missing.")?;
        let format = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        if capabilities.sequence_format != format {
            return Err("Device firmware and Dawn use different sequence formats. Rebuild the firmware and re-export together.".into());
        }
        if bytes.len() - 16 > capabilities.max_payload_bytes as usize {
            return Err(format!(
                "Sequence payload exceeds the device limit of {} bytes. Select fewer outputs or simplify the sequence.",
                capabilities.max_payload_bytes
            ));
        }
        match capabilities.output {
            DeviceOutputCapabilities::Ws281x { lanes, channels_per_lane, channel_multiple, .. } => {
                if channel_multiple == 0 || widths.len() > lanes as usize || widths.iter().any(|width| *width == 0 || u32::from(*width) > channels_per_lane || u32::from(*width) % channel_multiple != 0) {
                    return Err(format!("Device accepts at most {lanes} outputs, each with at most {channels_per_lane} channels in multiples of {channel_multiple}. Adjust the selected controller ports in Display Setup."));
                }
            }
            DeviceOutputCapabilities::EvaluationOnly => return Err("This firmware evaluates sequences but cannot drive lights. Install the output-enabled firmware before uploading from Dawn.".into()),
        }
        let response = self.client.put(format!("http://{}/sequence", self.address))
            .header("content-type", "application/octet-stream").body(bytes).send()
            .map_err(|error| format!("Upload did not finish: {error}. Check the device before retrying; it may have received the sequence."))?;
        let response = self.response(response)?;
        if !response.starts_with("LOADED ") {
            return Err("Device did not acknowledge loading the sequence.".into());
        }
        Ok("Sequence uploaded and playback started. It is saved on the device and will play from the beginning after a restart.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn transport_uses_authenticated_get_and_bodyless_posts_and_reads_device_state() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for (method, path, body) in [
                ("GET", "/transport", r#"{"playback":null}"#),
                (
                    "POST",
                    "/transport/play",
                    r#"{"playback":{"mode":"playing","positionMicros":0,"durationMicros":1000000}}"#,
                ),
                (
                    "POST",
                    "/transport/pause",
                    r#"{"playback":{"mode":"paused","positionMicros":500000,"durationMicros":1000000}}"#,
                ),
                (
                    "POST",
                    "/transport/stop",
                    r#"{"playback":{"mode":"stopped","positionMicros":0,"durationMicros":1000000}}"#,
                ),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert_eq!(line.trim(), format!("{method} {path} HTTP/1.1"));
                let mut authorized = false;
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    let (name, value) = line.trim().split_once(':').unwrap();
                    if name.eq_ignore_ascii_case("x-dawn-token") {
                        authorized = value.trim() == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
                    }
                    if name.eq_ignore_ascii_case("content-length") {
                        assert_eq!(value.trim(), "0");
                    }
                }
                assert!(authorized);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let client =
            DeviceClient::new(&address.to_string(), "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        assert!(client.transport(None).unwrap().playback.is_none());
        let playing = client
            .transport(Some(DevicePlaybackMode::Playing))
            .unwrap()
            .playback
            .unwrap();
        assert!(matches!(playing.mode, DevicePlaybackMode::Playing));
        let paused = client
            .transport(Some(DevicePlaybackMode::Paused))
            .unwrap()
            .playback
            .unwrap();
        assert!(matches!(paused.mode, DevicePlaybackMode::Paused));
        assert_eq!(paused.position_micros, 500_000);
        let stopped = client
            .transport(Some(DevicePlaybackMode::Stopped))
            .unwrap()
            .playback
            .unwrap();
        assert!(matches!(stopped.mode, DevicePlaybackMode::Stopped));
        assert_eq!(stopped.position_micros, 0);
        server.join().unwrap();
    }

    #[test]
    fn upload_checks_capabilities_before_sending_and_rejects_incompatible_ports() {
        for (width, should_upload) in [(90, true), (512, false)] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = thread::spawn(move || {
                for path in if should_upload {
                    vec!["/capabilities", "/sequence"]
                } else {
                    vec!["/capabilities"]
                } {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    assert_eq!(line.split_whitespace().nth(1), Some(path));
                    let mut length = 0;
                    let mut authorized = false;
                    loop {
                        line.clear();
                        reader.read_line(&mut line).unwrap();
                        if line == "\r\n" {
                            break;
                        }
                        let (name, value) = line.trim().split_once(':').unwrap();
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap();
                        }
                        if name.eq_ignore_ascii_case("x-dawn-token") {
                            authorized = value.trim() == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
                        }
                    }
                    assert!(authorized);
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    let response = if path == "/capabilities" {
                        r#"{"sequenceFormat":6,"maxPayloadBytes":32768,"maxPixels":1600,"maxGraphNodes":128,"maxWorkspaceBytes":98304,"output":{"type":"ws281x","lanes":4,"channelsPerLane":600,"channelMultiple":3,"frameRate":120},"sequenceStorage":"persistent"}"#
                    } else {
                        assert_eq!(&body[..4], b"DAWN");
                        "LOADED sequence"
                    };
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        response.len(),
                        response
                    )
                    .unwrap();
                }
            });
            let client =
                DeviceClient::new(&address.to_string(), "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                    .unwrap();
            let mut bytes = vec![0; 16];
            bytes[..4].copy_from_slice(b"DAWN");
            bytes[4..8].copy_from_slice(&6u32.to_le_bytes());
            let result = client.upload(bytes, &[width]);
            if should_upload {
                assert!(result.unwrap().contains("uploaded"));
            } else {
                assert!(result.unwrap_err().contains("multiples of 3"));
            }
            server.join().unwrap();
        }
    }
}
