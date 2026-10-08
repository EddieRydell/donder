//! Desktop clock master and prepared-show deployment; no pixel streaming.
use super::DeviceClient;
use crate::desktop_state::lock_unpoisoned;
use crate::dto::{DeviceOutputCapabilities, DevicePlaybackMode, PlaybackSpeed};
use donder_model::{ControllerId, ControllerPortId, DonderDeviceId, SequenceId};
use donder_runtime::PlaybackRate;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, hash_map::RandomState},
    hash::{BuildHasher, Hasher},
    net::{SocketAddr, UdpSocket},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(crate) type DevicePorts = Vec<(ControllerId, ControllerPortId)>;

/// A controller in the open setup that should follow editor playback.
pub(crate) struct WantedDevice {
    pub id: DonderDeviceId,
    pub address: SocketAddr,
    pub token: String,
    pub ports: DevicePorts,
}

#[derive(Clone, Copy, Debug)]
struct ClockSample {
    boot_id: u32,
    local: u64,
    master: u64,
    delay: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
enum Control {
    SyncClock {
        boot_id: u32,
        clock_id: u32,
        local_micros: u64,
        master_micros: u64,
        rate_ppb: i32,
        valid_for_micros: u32,
    },
    Schedule {
        boot_id: u32,
        clock_id: u32,
        command_id: u32,
        at_micros: u64,
        mode: DevicePlaybackMode,
        position_micros: u32,
        speed: PlaybackSpeed,
        looping: bool,
        archive_crc: u32,
        archive_bytes: u32,
    },
    Cancel {
        command_id: u32,
    },
}

fn clock_sample(t1: u64, t2: u64, t3: u64, t4: u64, boot_id: u32) -> Result<ClockSample, String> {
    let elapsed = t4.checked_sub(t1).ok_or("Desktop clock moved backwards")?;
    let processing = t3
        .checked_sub(t2)
        .ok_or("Controller clock moved backwards")?;
    let delay = elapsed
        .checked_sub(processing)
        .ok_or("Invalid clock exchange")?;
    Ok(ClockSample {
        boot_id,
        local: t2 + processing / 2,
        master: t1 + elapsed / 2,
        delay,
    })
}

fn udp_clock_sample(
    socket: &UdpSocket,
    token: &[u8; 32],
    origin: Instant,
) -> Result<ClockSample, String> {
    let t1 = origin.elapsed().as_micros() as u64;
    let mut request = [0; 44];
    request[..4].copy_from_slice(b"DCLK");
    request[4..36].copy_from_slice(token);
    request[36..].copy_from_slice(&t1.to_le_bytes());
    socket
        .send(&request)
        .map_err(|error| format!("UDP clock send failed: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("UDP clock reply timed out".into());
        }
        socket
            .set_read_timeout(Some(remaining))
            .map_err(|error| error.to_string())?;
        let mut reply = [0; 33];
        let length = socket
            .recv(&mut reply)
            .map_err(|error| format!("UDP clock receive failed: {error}"))?;
        let t4 = origin.elapsed().as_micros() as u64;
        if length != 32 || &reply[..4] != b"DCLK" {
            return Err("Malformed UDP clock reply".into());
        }
        if reply[4..12] != t1.to_le_bytes() {
            continue;
        }
        let read_u64 = |range: std::ops::Range<usize>| -> Result<u64, String> {
            Ok(u64::from_le_bytes(
                reply[range]
                    .try_into()
                    .map_err(|_| "Invalid clock timestamp")?,
            ))
        };
        let boot = u32::from_le_bytes(
            reply[12..16]
                .try_into()
                .map_err(|_| "Invalid clock identity")?,
        );
        return clock_sample(t1, read_u64(16..24)?, read_u64(24..32)?, t4, boot);
    }
}

impl DeviceClient {
    fn stop_loaded(&self) -> Result<(), String> {
        if self.transport(None)?.playback.is_some() {
            self.transport(Some(DevicePlaybackMode::Stopped))?;
        }
        Ok(())
    }
    fn control(&self, command: &Control) -> Result<(), String> {
        let body = serde_json::to_vec(command).map_err(|error| error.to_string())?;
        let response = self
            .client
            .post(format!("http://{}/control", self.address))
            .timeout(Duration::from_secs(2))
            .header("content-type", "application/json")
            .body(body)
            .send()
            .map_err(|error| format!("Controller command failed: {error}"))?;
        if self.response(response)?.trim() != "OK" {
            return Err("Controller did not acknowledge the command".into());
        }
        Ok(())
    }
}

struct Prepared {
    revision: u32,
    sequence: SequenceId,
    bytes: Vec<u8>,
    widths: Vec<u16>,
    hash: [u8; 32],
}
struct Binding {
    id: DonderDeviceId,
    client: DeviceClient,
    frame_rate: u32,
    clock_socket: UdpSocket,
    ports: DevicePorts,
    prepared: Option<Prepared>,
    uploaded: Option<[u8; 32]>,
    sample: Option<ClockSample>,
    rate_ppb: i32,
    uncertainty: Option<u32>,
    error: Option<String>,
}
struct Core {
    epoch: u32,
    bindings: Vec<Binding>,
    origin: Instant,
    clock_id: u32,
}
struct Shared {
    core: Mutex<Core>,
    stopping: Mutex<bool>,
    wake: Condvar,
}

pub(crate) struct DevicePlaybackService {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}
pub(crate) struct GroupSchedule {
    pub deadline: Instant,
    pub position: u32,
    commands: Vec<(SocketAddr, u32)>,
}

impl Binding {
    fn sample_clock(&self, origin: Instant) -> Result<ClockSample, String> {
        udp_clock_sample(&self.clock_socket, &self.client.token, origin)
    }

    fn synchronize(&mut self, origin: Instant, clock_id: u32) -> Result<(), String> {
        let mut best: Option<ClockSample> = None;
        for _ in 0..8 {
            let sample = self.sample_clock(origin)?;
            if best.is_some_and(|best| best.boot_id != sample.boot_id) {
                return Err("Controller restarted during clock synchronization".into());
            }
            if best.is_none_or(|best| sample.delay < best.delay) {
                best = Some(sample);
            }
        }
        let sample = best.ok_or("No clock samples")?;
        // Reported, not enforced: a slow network loosens light-to-audio sync
        // but must not prevent playback.
        let uncertainty = sample.delay / 2 + 100;
        let mut rate = self.rate_ppb;
        if let Some(previous) = self
            .sample
            .filter(|previous| previous.boot_id == sample.boot_id)
        {
            let elapsed = sample.local.saturating_sub(previous.local);
            if elapsed >= 30_000_000 {
                let master_elapsed = i128::from(sample.master) - i128::from(previous.master);
                let estimate =
                    (master_elapsed - i128::from(elapsed)) * 1_000_000_000 / i128::from(elapsed);
                if estimate.abs() > 200_000 {
                    return Err("Measured clock drift exceeds the accepted range".into());
                }
                rate = estimate as i32;
            }
        }
        self.client.control(&Control::SyncClock {
            boot_id: sample.boot_id,
            clock_id,
            local_micros: sample.local,
            master_micros: sample.master,
            rate_ppb: rate,
            valid_for_micros: 15_000_000,
        })?;
        if self.sample.is_none_or(|previous| {
            previous.boot_id != sample.boot_id
                || sample.local.saturating_sub(previous.local) >= 30_000_000
        }) {
            self.sample = Some(sample);
        }
        self.rate_ppb = rate;
        self.uncertainty = Some(uncertainty as u32);
        self.error = None;
        Ok(())
    }
}

impl DevicePlaybackService {
    pub(crate) fn new(report_error: impl Fn(String) + Send + 'static) -> Self {
        let shared = Arc::new(Shared {
            core: Mutex::new(Core {
                epoch: 0,
                bindings: Vec::new(),
                origin: Instant::now(),
                clock_id: (RandomState::new().build_hasher().finish() as u32).max(1),
            }),
            stopping: Mutex::new(false),
            wake: Condvar::new(),
        });
        let background = shared.clone();
        let worker = thread::spawn(move || {
            loop {
                let stopping = lock_unpoisoned(&background.stopping);
                let (stopping, _) = background
                    .wake
                    .wait_timeout(stopping, Duration::from_secs(5))
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if *stopping {
                    break;
                }
                drop(stopping);
                let mut core = lock_unpoisoned(&background.core);
                let origin = core.origin;
                let clock_id = core.clock_id;
                let mut reports = Vec::new();
                for binding in &mut core.bindings {
                    if let Err(error) = binding.synchronize(origin, clock_id) {
                        if binding.error.as_ref() != Some(&error) {
                            reports.push(format!(
                                "Controller {}: {error}. Running playback continues locally.",
                                binding.client.address
                            ));
                        }
                        binding.error = Some(error);
                    }
                }
                drop(core);
                for report in reports {
                    report_error(report);
                }
            }
        });
        Self {
            shared,
            worker: Some(worker),
        }
    }
    /// Match bindings to the controllers the open setup wants. Unchanged
    /// bindings keep their clock state and uploaded show; new ones stop the
    /// controller's standalone show and synchronize clocks.
    pub(crate) fn sync(
        &self,
        epoch: u32,
        wanted: Vec<WantedDevice>,
    ) -> BTreeMap<DonderDeviceId, Result<(), String>> {
        let mut core = lock_unpoisoned(&self.shared.core);
        if core.epoch != epoch {
            core.epoch = epoch;
            for binding in std::mem::take(&mut core.bindings) {
                // An unreachable controller cannot be playing this editor's show.
                let _ = binding.client.stop_loaded();
            }
        }
        for binding in std::mem::take(&mut core.bindings) {
            match wanted.iter().find(|device| device.id == binding.id) {
                Some(device)
                    if device.address == binding.client.address
                        && device.ports == binding.ports =>
                {
                    core.bindings.push(binding);
                }
                Some(_) => {}
                None => {
                    let _ = binding.client.stop_loaded();
                }
            }
        }
        let mut results = BTreeMap::new();
        for device in wanted {
            let result = if core.bindings.iter().any(|binding| binding.id == device.id) {
                Ok(())
            } else {
                bind(&core, &device).map(|binding| core.bindings.push(binding))
            };
            results.insert(device.id, result);
        }
        results
    }
    pub(crate) fn connections(&self) -> BTreeMap<DonderDeviceId, Result<Option<u32>, String>> {
        lock_unpoisoned(&self.shared.core)
            .bindings
            .iter()
            .map(|binding| {
                (
                    binding.id.clone(),
                    binding.error.clone().map_or(Ok(binding.uncertainty), Err),
                )
            })
            .collect()
    }
    pub(crate) fn has_devices(&self) -> bool {
        !lock_unpoisoned(&self.shared.core).bindings.is_empty()
    }
    pub(crate) fn stop_for_source(
        &self,
        epoch: u32,
        sequence: Option<&SequenceId>,
    ) -> Result<(), String> {
        let mut core = lock_unpoisoned(&self.shared.core);
        let changed = core.epoch != epoch
            || sequence.is_none()
            || core.bindings.iter().any(|binding| {
                binding
                    .prepared
                    .as_ref()
                    .is_some_and(|prepared| Some(&prepared.sequence) != sequence)
            });
        if !changed {
            return Ok(());
        }
        let mut errors = Vec::new();
        for binding in &mut core.bindings {
            if let Err(error) = binding.client.stop_loaded() {
                errors.push(format!("{}: {error}", binding.client.address));
            }
            binding.prepared = None;
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Could not confirm devices stopped: {}",
                errors.join("; ")
            ))
        }
    }
    pub(crate) fn prepare(
        &self,
        epoch: u32,
        revision: u32,
        sequence: &SequenceId,
        mut compile: impl FnMut(&DevicePorts) -> Result<(Vec<u8>, Vec<u16>), String>,
    ) -> Result<(), String> {
        let mut core = lock_unpoisoned(&self.shared.core);
        if core.epoch != epoch {
            return Err(
                "Connected devices belong to a different project. Disconnect them first.".into(),
            );
        }
        for binding in &mut core.bindings {
            if binding.prepared.as_ref().is_none_or(|prepared| {
                prepared.revision != revision || prepared.sequence != *sequence
            }) {
                let (bytes, widths) = compile(&binding.ports)?;
                let hash = Sha256::digest(&bytes).into();
                binding.prepared = Some(Prepared {
                    revision,
                    sequence: sequence.clone(),
                    bytes,
                    widths,
                    hash,
                });
            }
            let prepared = binding
                .prepared
                .as_ref()
                .ok_or("Prepared sequence is missing")?;
            let status = binding.client.transport(None)?;
            let checksum = u32::from_le_bytes(
                prepared
                    .bytes
                    .get(12..16)
                    .ok_or("Prepared archive header is missing")?
                    .try_into()
                    .map_err(|_| "Invalid prepared archive header")?,
            );
            let matches = status.playback.as_ref().is_some_and(|playback| {
                playback.archive_bytes as usize == prepared.bytes.len()
                    && playback.archive_crc == checksum
            });
            if binding.uploaded != Some(prepared.hash) || !matches {
                binding
                    .client
                    .upload(prepared.bytes.clone(), &prepared.widths)?;
                binding.uploaded = Some(prepared.hash);
            }
        }
        Ok(())
    }
    /// `advancing` is the rate `position` is moving at as of `observed_at`;
    /// `rate` is the playback rate the devices follow after the deadline.
    pub(crate) fn schedule(
        &self,
        mode: DevicePlaybackMode,
        position: u32,
        advancing: Option<PlaybackRate>,
        rate: PlaybackRate,
        duration: u32,
        observed_at: Instant,
    ) -> Result<GroupSchedule, String> {
        let mut core = lock_unpoisoned(&self.shared.core);
        let origin = core.origin;
        let clock_id = core.clock_id;
        let mut samples = Vec::new();
        for binding in &mut core.bindings {
            binding.synchronize(origin, clock_id)?;
            let sample = binding.sample_clock(origin)?;
            let status = binding
                .client
                .transport(None)?
                .playback
                .ok_or("Controller has no loaded sequence")?;
            let prepared = binding
                .prepared
                .as_ref()
                .ok_or("Prepare the current sequence before scheduling")?;
            let crc = u32::from_le_bytes(
                prepared
                    .bytes
                    .get(12..16)
                    .ok_or("Prepared archive header is missing")?
                    .try_into()
                    .map_err(|_| "Invalid prepared archive header")?,
            );
            if status.archive_crc != crc || status.archive_bytes as usize != prepared.bytes.len() {
                return Err(format!(
                    "Controller {} has a different show loaded; press Play to upload the current sequence",
                    binding.client.address
                ));
            }
            samples.push((sample, status));
        }
        // Each schedule acknowledgment costs a few network round trips; slow
        // links get a later start instead of a missed deadline.
        let slowest = core
            .bindings
            .iter()
            .filter_map(|binding| binding.uncertainty)
            .max()
            .unwrap_or(0);
        let lead = Duration::from_millis((core.bindings.len() as u64 * 50).clamp(250, 1_000))
            + Duration::from_micros(u64::from(slowest) * 8 * core.bindings.len() as u64);
        let frame_rate = u128::from(
            core.bindings
                .first()
                .ok_or("No connected devices")?
                .frame_rate,
        );
        let earliest = origin.elapsed().as_micros() + lead.as_micros();
        let frame = (earliest * frame_rate).div_ceil(1_000_000);
        let at = u64::try_from((frame * 1_000_000).div_ceil(frame_rate))
            .map_err(|_| "Playback clock overflow")?;
        let deadline = origin + Duration::from_micros(at);
        let position = match advancing {
            Some(current) => (u64::from(position)
                + current.show_elapsed(
                    deadline.saturating_duration_since(observed_at).as_micros() as u64
                ))
            .min(u64::from(duration)) as u32,
            None => position.min(duration),
        };
        let mut commands = Vec::new();
        for (binding, (sample, status)) in core.bindings.iter().zip(samples) {
            let command_id = status
                .command_id
                .checked_add(1)
                .ok_or("Controller command counter exhausted; restart the controller")?;
            let command = Control::Schedule {
                boot_id: sample.boot_id,
                clock_id,
                command_id,
                at_micros: at,
                mode,
                position_micros: position,
                speed: rate.into(),
                looping: false,
                archive_crc: status.archive_crc,
                archive_bytes: status.archive_bytes,
            };
            // Include the current target in cancellation: a lost reply may hide acceptance.
            commands.push((binding.client.address, command_id));
            if let Err(error) = binding.client.control(&command) {
                let cancellation = cancel(&core, &commands);
                return Err(format!(
                    "Could not schedule controller {}: {error}. {cancellation}",
                    binding.client.address
                ));
            }
        }
        if deadline.saturating_duration_since(Instant::now()) < Duration::from_millis(40) {
            let cancellation = cancel(&core, &commands);
            return Err(format!(
                "Scheduled deadline became too close. {cancellation}"
            ));
        }
        Ok(GroupSchedule {
            deadline,
            position,
            commands,
        })
    }
    pub(crate) fn cancel(&self, schedule: &GroupSchedule) -> String {
        cancel(&lock_unpoisoned(&self.shared.core), &schedule.commands)
    }
}

fn cancel(core: &Core, commands: &[(SocketAddr, u32)]) -> String {
    let mut failed = Vec::new();
    for (address, command_id) in commands {
        if let Some(binding) = core
            .bindings
            .iter()
            .find(|binding| binding.client.address == *address)
            && let Err(error) = binding.client.control(&Control::Cancel {
                command_id: *command_id,
            })
        {
            failed.push(format!("{address}: {error}"));
        }
    }
    if failed.is_empty() {
        "Pending starts cancelled.".into()
    } else {
        format!(
            "Cancellation could not be confirmed: {}. Check device playback status.",
            failed.join("; ")
        )
    }
}
fn bind(core: &Core, device: &WantedDevice) -> Result<Binding, String> {
    let client = DeviceClient::new(device.address, &device.token)?;
    let (frame_rate, clock_udp_port) = match client.capabilities()?.output {
        DeviceOutputCapabilities::Ws281x {
            frame_rate,
            clock_udp_port,
            ..
        } if frame_rate > 0 && clock_udp_port > 0 => (frame_rate, clock_udp_port),
        _ => return Err("Device does not advertise a valid physical output frame rate".into()),
    };
    if core
        .bindings
        .iter()
        .any(|binding| binding.frame_rate != frame_rate)
    {
        return Err("Grouped devices must use the same output frame rate".into());
    }
    let bind_address = if client.address.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let clock_socket =
        UdpSocket::bind(bind_address).map_err(|error| format!("Clock socket: {error}"))?;
    clock_socket
        .connect(SocketAddr::new(client.address.ip(), clock_udp_port))
        .map_err(|error| format!("Clock endpoint: {error}"))?;
    clock_socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    clock_socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    // Changing clock masters while an old standalone show runs would move its time origin.
    client.stop_loaded()?;
    let mut binding = Binding {
        id: device.id.clone(),
        client,
        clock_socket,
        frame_rate,
        ports: device.ports.clone(),
        prepared: None,
        uploaded: None,
        sample: None,
        rate_ppb: 0,
        uncertainty: None,
        error: None,
    };
    binding.synchronize(core.origin, core.clock_id)?;
    Ok(binding)
}
impl Drop for DevicePlaybackService {
    fn drop(&mut self) {
        *lock_unpoisoned(&self.shared.stopping) = true;
        self.shared.wake.notify_all();
        if let Some(worker) = self.worker.take()
            && worker.thread().id() != thread::current().id()
        {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn udp_clock_exchange_authenticates_and_echoes_nonce() {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let mut request = [0; 45];
            let (length, peer) = server.recv_from(&mut request).unwrap();
            assert_eq!(length, 44);
            assert_eq!(&request[..4], b"DCLK");
            assert_eq!(&request[4..36], &[b'a'; 32]);
            let mut reply = [0; 32];
            reply[..4].copy_from_slice(b"DCLK");
            reply[4..12].copy_from_slice(&request[36..44]);
            reply[12..16].copy_from_slice(&7u32.to_le_bytes());
            reply[16..24].copy_from_slice(&600000u64.to_le_bytes());
            reply[24..32].copy_from_slice(&600000u64.to_le_bytes());
            let mut stale = reply;
            stale[4] ^= 1;
            server.send_to(&stale, peer).unwrap();
            server.send_to(&reply, peer).unwrap();
        });
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.connect(address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let sample = udp_clock_sample(&socket, &[b'a'; 32], Instant::now()).unwrap();
        assert_eq!(sample.boot_id, 7);
        assert_eq!(sample.local, 600000);
        worker.join().unwrap();
    }
    #[test]
    fn four_timestamps_remove_reply_processing_and_find_offset() {
        let sample = clock_sample(100_000, 601_000, 604_000, 105_000, 1).unwrap();
        assert_eq!(sample.delay, 2_000);
        assert_eq!(sample.local - sample.master, 500_000);
        assert!(clock_sample(100, 200, 190, 110, 1).is_err());
        assert!(clock_sample(100, 200, 220, 110, 1).is_err());
    }
}
