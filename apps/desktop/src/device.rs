pub(crate) mod console;
pub(crate) mod discovery;
pub(crate) mod playback;

use donder_device_protocol::{LOADED, TransportStatus, path};
use donder_editor::dto::DonderDeviceNetworkRequest;
use reqwest::blocking::Client;
use std::{io::Read, net::SocketAddr, time::Duration};

pub(crate) struct DeviceClient {
    client: Client,
    address: SocketAddr,
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
    pub(crate) fn new(address: SocketAddr) -> Result<Self, String> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| format!("Could not initialize device connection: {error}"))?;
        Ok(Self { client, address })
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    fn response(&self, response: reqwest::blocking::Response) -> Result<String, String> {
        read_response(response)
    }

    pub(crate) fn rename(&self, name: &str) -> Result<(), String> {
        if !donder_device_protocol::valid_name(name) {
            return Err(format!(
                "A controller name is 1-{} bytes without control characters.",
                donder_device_protocol::MAX_NAME_BYTES
            ));
        }
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
            Some(network) => {
                let credentials = donder_device_protocol::NetworkCredentials {
                    ssid: network.ssid.clone(),
                    password: network.password.clone(),
                };
                if !credentials.is_valid() {
                    return Err(format!(
                        "A Wi-Fi network needs a {}-{} byte name and a {}-{} byte WPA2 password.",
                        donder_device_protocol::NetworkCredentials::SSID_BYTES.start(),
                        donder_device_protocol::NetworkCredentials::SSID_BYTES.end(),
                        donder_device_protocol::NetworkCredentials::PASSWORD_BYTES.start(),
                        donder_device_protocol::NetworkCredentials::PASSWORD_BYTES.end(),
                    ));
                }
                serde_json::to_vec(&credentials).map_err(|error| error.to_string())?
            }
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

    /// The loaded show's playback, after stopping it when `stop`.
    pub(crate) fn transport(&self, stop: bool) -> Result<TransportStatus, String> {
        let request = if stop {
            self.client.post(self.url(path::STOP)).body(Vec::new())
        } else {
            self.client.get(self.url(path::TRANSPORT))
        };
        let response = request.send().map_err(|error| format!("Device transport request did not finish: {error}. Refresh device status before retrying."))?;
        serde_json::from_str(&self.response(response)?)
            .map_err(|error| format!("Device returned invalid playback status: {error}"))
    }

    fn upload(&self, bytes: Vec<u8>) -> Result<(), String> {
        let response = self
            .client
            .put(self.url(path::SHOW))
            .header("content-type", "application/octet-stream")
            .body(bytes)
            .send()
            .map_err(|error| format!("Could not send the show to the controller: {error}"))?;
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
            return Err("Controller did not acknowledge the show.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use donder_device_protocol::Mode;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn transport_uses_get_and_a_bodyless_stop_and_reads_device_state() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for (method, path, body) in [
                ("GET", "/transport", r#"{"playback":null}"#),
                (
                    "POST",
                    "/transport/stop",
                    r#"{"playback":{"mode":"stopped","positionMicros":0,"showId":0,"showBytes":0,"commandId":0}}"#,
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
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    let (name, value) = line.trim().split_once(':').unwrap();
                    if name.eq_ignore_ascii_case("content-length") {
                        assert_eq!(value.trim(), "0");
                    }
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let client = DeviceClient::new(address).unwrap();
        assert!(client.transport(false).unwrap().playback.is_none());
        let stopped = client.transport(true).unwrap().playback.unwrap();
        assert!(matches!(stopped.mode, Mode::Stopped));
        assert_eq!(stopped.position_micros, 0);
        server.join().unwrap();
    }
}
