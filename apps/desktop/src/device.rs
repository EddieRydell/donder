pub(crate) mod discovery;
pub(crate) mod playback;
pub(crate) mod provisioning;

use donder_device_protocol::{
    Capabilities, LOADED, Mode, OutputCapabilities, TOKEN_HEADER, TransportStatus, path,
};
use donder_editor::dto::DonderDeviceNetworkRequest;
use reqwest::{
    blocking::Client,
    header::{HeaderMap, HeaderValue},
};
use std::{io::Read, net::SocketAddr, time::Duration};

pub(crate) struct DeviceClient {
    client: Client,
    address: SocketAddr,
    token: [u8; 32],
}

/// First claim wins: an unclaimed controller returns the token that
/// authorizes every later request.
pub(crate) fn claim(address: SocketAddr) -> Result<String, String> {
    let response = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| format!("Could not initialize device connection: {error}"))?
        .post(format!("http://{address}{}", path::CLAIM))
        .body(Vec::new())
        .send()
        .map_err(|error| format!("Could not contact device: {error}"))?;
    let token = read_response(response)?;
    if token.len() != 32 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Device returned an invalid claim token.".into());
    }
    Ok(token)
}

fn read_response(response: reqwest::blocking::Response) -> Result<String, String> {
    let status = response.status();
    let text = read_body(response)?;
    if !status.is_success() {
        return Err(format!("Device returned HTTP {status}: {}", text.trim()));
    }
    Ok(text)
}

fn read_body(response: reqwest::blocking::Response) -> Result<String, String> {
    let mut bytes = Vec::new();
    response
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read device response: {error}"))?;
    if bytes.len() > 8192 {
        return Err("Device response exceeded its expected size.".into());
    }
    String::from_utf8(bytes).map_err(|_| "Device returned invalid response text.".into())
}

impl DeviceClient {
    pub(crate) fn new(address: SocketAddr, token: &str) -> Result<Self, String> {
        if token.len() != 32 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Saved device token is invalid; claim the controller again.".into());
        }
        let mut value = HeaderValue::from_str(token).map_err(|_| "Invalid device token.")?;
        value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(TOKEN_HEADER, value);
        let client = Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| format!("Could not initialize device connection: {error}"))?;
        let token = token
            .as_bytes()
            .try_into()
            .map_err(|_| "Invalid device token size")?;
        Ok(Self {
            client,
            address,
            token,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    fn response(&self, response: reqwest::blocking::Response) -> Result<String, String> {
        read_response(response)
    }

    pub(crate) fn rename(&self, name: &str) -> Result<(), String> {
        let response = self
            .client
            .put(self.url(path::NAME))
            .body(name.to_string())
            .send()
            .map_err(|error| format!("Could not contact device: {error}"))?;
        self.response(response).map(|_| ())
    }

    /// Join `network` after the controller restarts, or return to its own
    /// access point when `None`.
    pub(crate) fn set_network(
        &self,
        network: Option<&DonderDeviceNetworkRequest>,
    ) -> Result<(), String> {
        let body = match network {
            Some(network) => serde_json::to_vec(network).map_err(|error| error.to_string())?,
            None => Vec::new(),
        };
        let response = self
            .client
            .put(self.url(path::NETWORK))
            .header("content-type", "application/json")
            .body(body)
            .send()
            .map_err(|error| format!("Could not contact device: {error}"))?;
        self.response(response).map(|_| ())
    }

    pub(crate) fn capabilities(&self) -> Result<Capabilities, String> {
        let response = self
            .client
            .get(self.url(path::CAPABILITIES))
            .send()
            .map_err(|error| format!("Could not contact device: {error}"))?;
        serde_json::from_str(&self.response(response)?)
            .map_err(|error| format!("Device returned invalid capabilities: {error}"))
    }

    pub(crate) fn transport(&self, mode: Option<Mode>) -> Result<TransportStatus, String> {
        let request = match mode {
            None => self.client.get(self.url(path::TRANSPORT)),
            Some(mode) => {
                let command = match mode {
                    Mode::Playing => path::PLAY,
                    Mode::Paused => path::PAUSE,
                    Mode::Stopped => path::STOP,
                    Mode::Ended => {
                        return Err("Ended is a playback status, not a transport command.".into());
                    }
                };
                self.client.post(self.url(command)).body(Vec::new())
            }
        };
        let response = request.send().map_err(|error| format!("Device transport request did not finish: {error}. Refresh device status before retrying."))?;
        serde_json::from_str(&self.response(response)?)
            .map_err(|error| format!("Device returned invalid playback status: {error}"))
    }

    pub(crate) fn upload(
        &self,
        bytes: Vec<u8>,
        widths: &[u16],
        frame_rate: u32,
    ) -> Result<String, String> {
        let capabilities = self.capabilities()?;
        let header: &[u8; 16] = bytes
            .get(..16)
            .and_then(|header| header.try_into().ok())
            .ok_or("Compiled sequence header is missing.")?;
        let format = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        if capabilities.image_format != format {
            return Err("Device firmware and Donder use different sequence formats. Rebuild the firmware and re-export together.".into());
        }
        if bytes.len() - 16 > capabilities.max_payload_bytes as usize {
            return Err(format!(
                "Sequence payload exceeds the device limit of {} bytes. Select fewer outputs or simplify the sequence.",
                capabilities.max_payload_bytes
            ));
        }
        match capabilities.output {
            OutputCapabilities::Ws281x { lanes, channels_per_lane, channel_multiple, max_frame_rate, .. } => {
                if frame_rate > max_frame_rate {
                    return Err(format!("This sequence runs at {frame_rate} frames per second; this controller's outputs carry at most {max_frame_rate}. Lower the sequence's frame rate."));
                }
                if channel_multiple == 0 || widths.len() > lanes as usize || widths.iter().any(|width| *width == 0 || u32::from(*width) > channels_per_lane || u32::from(*width) % channel_multiple != 0) {
                    return Err(format!("Device accepts at most {lanes} outputs, each with at most {channels_per_lane} channels in multiples of {channel_multiple}. Adjust the selected controller ports in Display Setup."));
                }
            }
            OutputCapabilities::EvaluationOnly => return Err("This firmware evaluates sequences but cannot drive lights. Install the output-enabled firmware before uploading from Donder.".into()),
        }
        let response = self.client.put(self.url(path::IMAGE))
            .header("content-type", "application/octet-stream").body(bytes).send()
            .map_err(|error| format!("Upload did not finish: {error}. Check the device before retrying; it may have received the sequence."))?;
        // The controller admits a show only if it fits; the same show is
        // refused again however often it is sent.
        if response.status() == reqwest::StatusCode::UNPROCESSABLE_ENTITY {
            let reason = read_body(response)?;
            return Err(format!(
                "Controller {} rejected this show: {}",
                self.address,
                reason.trim()
            ));
        }
        let response = self.response(response)?;
        if !response.starts_with(LOADED) {
            return Err("Device did not acknowledge loading the sequence.".into());
        }
        Ok("Sequence uploaded and saved on the device. Press Play to start; uploads and restarts leave playback stopped.".into())
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
                    r#"{"playback":{"mode":"playing","positionMicros":0,"durationMicros":1000000,"imageId":0,"imageBytes":0,"pendingCommand":null,"commandId":0}}"#,
                ),
                (
                    "POST",
                    "/transport/pause",
                    r#"{"playback":{"mode":"paused","positionMicros":500000,"durationMicros":1000000,"imageId":0,"imageBytes":0,"pendingCommand":null,"commandId":0}}"#,
                ),
                (
                    "POST",
                    "/transport/stop",
                    r#"{"playback":{"mode":"stopped","positionMicros":0,"durationMicros":1000000,"imageId":0,"imageBytes":0,"pendingCommand":null,"commandId":0}}"#,
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
                    if name.eq_ignore_ascii_case("x-donder-token") {
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
        let client = DeviceClient::new(address, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        assert!(client.transport(None).unwrap().playback.is_none());
        let playing = client
            .transport(Some(Mode::Playing))
            .unwrap()
            .playback
            .unwrap();
        assert!(matches!(playing.mode, Mode::Playing));
        let paused = client
            .transport(Some(Mode::Paused))
            .unwrap()
            .playback
            .unwrap();
        assert!(matches!(paused.mode, Mode::Paused));
        assert_eq!(paused.position_micros, 500_000);
        let stopped = client
            .transport(Some(Mode::Stopped))
            .unwrap()
            .playback
            .unwrap();
        assert!(matches!(stopped.mode, Mode::Stopped));
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
                    vec!["/capabilities", "/image"]
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
                        if name.eq_ignore_ascii_case("x-donder-token") {
                            authorized = value.trim() == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
                        }
                    }
                    assert!(authorized);
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    let response = if path == "/capabilities" {
                        r#"{"imageFormat":6,"maxPayloadBytes":32768,"maxPixels":1600,"maxGraphNodes":128,"maxWorkspaceBytes":98304,"output":{"type":"ws281x","lanes":4,"channelsPerLane":600,"channelMultiple":3,"maxFrameRate":158,"clockUdpPort":80}}"#
                    } else {
                        assert_eq!(&body[..4], &6u32.to_le_bytes());
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
            let client = DeviceClient::new(address, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
            let mut bytes = vec![0; 16];
            bytes[..4].copy_from_slice(&6u32.to_le_bytes());
            let result = client.upload(bytes, &[width], 60);
            if should_upload {
                assert!(result.unwrap().contains("uploaded"));
            } else {
                assert!(result.unwrap_err().to_string().contains("multiples of 3"));
            }
            server.join().unwrap();
        }
    }
}
