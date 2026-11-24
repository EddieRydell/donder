//! Desktop clock master and prepared-show deployment; no pixel streaming.
use super::DeviceClient;
use crate::desktop_state::lock_unpoisoned;
use donder_device_protocol::{Action, Command, clock};
use donder_model::{ControllerId, ControllerOutput, DonderDeviceId};
use donder_project_io::ObjectAddress;
use donder_runtime::{PlaybackRate, PreparedShow};
use std::{
    collections::{BTreeMap, hash_map::RandomState},
    hash::{BuildHasher, Hasher},
    net::{SocketAddr, UdpSocket},
    num::NonZeroU32,
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(crate) type DevicePorts = Vec<(ControllerId, ControllerOutput)>;
/// Controllers released because they could not take part, and why.
pub(crate) type DeviceFailures = BTreeMap<DonderDeviceId, String>;

/// A controller in the open setup that should follow editor playback.
pub(crate) struct WantedDevice {
    pub id: DonderDeviceId,
    pub address: SocketAddr,
    pub ports: DevicePorts,
}

#[derive(Clone, Copy, Debug)]
struct ClockSample {
    boot_id: u32,
    local: u64,
    master: u64,
    delay: u64,
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

fn udp_clock_sample(socket: &UdpSocket, origin: Instant) -> Result<ClockSample, String> {
    let t1 = origin.elapsed().as_micros() as u64;
    let request = clock::Request { sent_micros: t1 };
    socket
        .send(&request.encode())
        .map_err(|error| format!("UDP clock send failed: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("UDP clock reply timed out".into());
        }
        socket
            .set_read_timeout(Some(remaining))
            .map_err(|error| format!("UDP clock timeout could not be set: {error}"))?;
        let mut reply = [0; clock::Reply::BYTES + 1];
        let length = socket
            .recv(&mut reply)
            .map_err(|error| format!("UDP clock receive failed: {error}"))?;
        let t4 = origin.elapsed().as_micros() as u64;
        let reply = clock::Reply::decode(&reply[..length]).ok_or("Malformed UDP clock reply")?;
        if reply.sent_micros != t1 {
            continue;
        }
        return clock_sample(
            t1,
            reply.received_micros,
            reply.replied_micros,
            t4,
            reply.boot_id,
        );
    }
}

impl DeviceClient {
    fn stop_loaded(&self) -> Result<(), String> {
        self.transport(true).map(|_| ())
    }
    fn control(&self, command: &Command) -> Result<(), String> {
        let body = serde_json::to_vec(command).map_err(|error| error.to_string())?;
        let response = self
            .client
            .post(self.url(donder_device_protocol::path::CONTROL))
            .timeout(Duration::from_secs(2))
            .header("content-type", "application/json")
            .body(body)
            .send()
            .map_err(|error| format!("Controller command failed: {error}"))?;
        if self.response(response)?.trim() != donder_device_protocol::OK {
            return Err("Controller did not acknowledge the command".into());
        }
        Ok(())
    }
}

struct Prepared {
    revision: u32,
    /// The prepared sequence's address, which outlives reloads.
    sequence: ObjectAddress,
    bytes: Vec<u8>,
    /// The show's identity, which the controller reports for the show it holds.
    id: u32,
}
/// A show a controller holds.
struct Held {
    id: u32,
    bytes: Vec<u8>,
}

struct Binding {
    id: DonderDeviceId,
    client: DeviceClient,
    clock_socket: UdpSocket,
    ports: DevicePorts,
    prepared: Option<Prepared>,
    /// The show the controller holds, which a delta changes.
    held: Option<Held>,
    sample: Option<ClockSample>,
    rate_ppb: i32,
    uncertainty: Option<u32>,
    /// The slowest recent command round trip, decaying toward the latest.
    round_trip: Duration,
    /// The last command sent; the controller accepts only later ones.
    command_id: u32,
    error: Option<String>,
}
struct Core {
    epoch: u32,
    bindings: Vec<Binding>,
    origin: Instant,
    clock_id: NonZeroU32,
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
/// The controllers that took a schedule, and those released because they could not.
pub(crate) struct Scheduled {
    addresses: Vec<SocketAddr>,
    pub failures: DeviceFailures,
}

impl Scheduled {
    /// The addresses of the controllers that follow the schedule.
    pub(crate) fn addresses(&self) -> impl Iterator<Item = SocketAddr> + '_ {
        self.addresses.iter().copied()
    }
}

impl Binding {
    /// Prepare the show for this controller's ports, for another sequence or
    /// a later revision, and send it unless the controller holds it: as a
    /// delta of the show it holds, which keeps playing, when that is less
    /// than half the show.
    fn prepare(
        &mut self,
        revision: u32,
        sequence: &ObjectAddress,
        compile: &mut impl FnMut(&DevicePorts) -> Result<PreparedShow, String>,
    ) -> Result<(), String> {
        let prepared = match &mut self.prepared {
            Some(prepared) if prepared.sequence == *sequence && prepared.revision == revision => {
                prepared
            }
            slot => {
                let show = compile(&self.ports)?;
                if let Some(reason) = super::show_size::too_large(&show.size()) {
                    return Err(reason);
                }
                slot.insert(Prepared {
                    revision,
                    sequence: sequence.clone(),
                    bytes: show.encode(),
                    id: show.id(),
                })
            }
        };
        let prepared = &*prepared;
        if self
            .held
            .as_ref()
            .is_some_and(|held| held.id == prepared.id)
        {
            return Ok(());
        }
        let delta = self.held.as_ref().map(|held| {
            donder_device_protocol::delta::diff(&held.bytes, held.id, &prepared.bytes, prepared.id)
        });
        // A failed send leaves the controller's show unknown.
        self.held = None;
        let updated = match delta {
            Some(delta) if delta.len() <= prepared.bytes.len() / 2 => self.client.update(delta)?,
            _ => false,
        };
        if !updated {
            self.client.upload(prepared.bytes.clone())?;
        }
        self.held = Some(Held {
            id: prepared.id,
            bytes: prepared.bytes.clone(),
        });
        Ok(())
    }

    fn schedule(
        &mut self,
        clock_id: NonZeroU32,
        at_micros: u64,
        action: Action,
        position_micros: u32,
        rate: PlaybackRate,
    ) -> Result<(), String> {
        let prepared = self
            .prepared
            .as_ref()
            .ok_or("Prepare the show before scheduling it")?;
        let Ok(show_bytes) = u32::try_from(prepared.bytes.len()) else {
            unreachable!("a prepared show fits a controller's show region")
        };
        let sample = self
            .sample
            .ok_or("The controller's clock is not synchronized")?;
        // A lost reply may hide acceptance, so the next command counts past this one either way.
        self.command_id = self
            .command_id
            .checked_add(1)
            .ok_or("Controller command counter exhausted; restart the controller")?;
        self.client.control(&Command::Schedule {
            boot_id: sample.boot_id,
            clock_id,
            command_id: self.command_id,
            at_micros,
            action,
            position_micros,
            rate,
            show_id: prepared.id,
            show_bytes,
        })
    }

    fn sample_clock(&self, origin: Instant) -> Result<ClockSample, String> {
        udp_clock_sample(&self.clock_socket, origin)
    }

    fn synchronize(&mut self, origin: Instant, clock_id: NonZeroU32) -> Result<(), String> {
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
        // A restarted controller holds no show.
        if self
            .sample
            .is_some_and(|previous| previous.boot_id != sample.boot_id)
        {
            self.held = None;
        }
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
        let sent = Instant::now();
        self.client.control(&Command::SyncClock {
            boot_id: sample.boot_id,
            clock_id,
            local_micros: sample.local,
            master_micros: sample.master,
            rate_ppb: rate,
            valid_for_micros: 15_000_000,
        })?;
        self.round_trip = sent
            .elapsed()
            .max(self.round_trip.saturating_sub(self.round_trip / 8));
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
                clock_id: NonZeroU32::MIN
                    .saturating_add(RandomState::new().build_hasher().finish() as u32),
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
    /// bindings keep their clock state and uploaded show; new ones stop
    /// whatever the controller plays and synchronize clocks.
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
    /// The addresses of the connected controllers.
    pub(crate) fn addresses(&self) -> Vec<SocketAddr> {
        lock_unpoisoned(&self.shared.core)
            .bindings
            .iter()
            .map(|binding| binding.client.address)
            .collect()
    }

    pub(crate) fn has_devices(&self) -> bool {
        !lock_unpoisoned(&self.shared.core).bindings.is_empty()
    }
    pub(crate) fn stop_for_source(
        &self,
        epoch: u32,
        sequence: Option<&ObjectAddress>,
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
    /// Build and upload the show for every binding. A controller that
    /// cannot take it is released and named in the result; the rest play.
    pub(crate) fn prepare(
        &self,
        epoch: u32,
        revision: u32,
        sequence: &ObjectAddress,
        mut compile: impl FnMut(&DevicePorts) -> Result<PreparedShow, String>,
    ) -> Result<DeviceFailures, String> {
        let mut core = lock_unpoisoned(&self.shared.core);
        if core.epoch != epoch {
            return Err(
                "Connected devices belong to a different project. Disconnect them first.".into(),
            );
        }
        let mut failures = DeviceFailures::new();
        for mut binding in std::mem::take(&mut core.bindings) {
            match binding.prepare(revision, sequence, &mut compile) {
                Ok(()) => core.bindings.push(binding),
                Err(error) => {
                    let _ = binding.client.stop_loaded();
                    failures.insert(binding.id, error);
                }
            }
        }
        Ok(failures)
    }

    /// The earliest moment a schedule sent now reaches every controller in
    /// time: the controller's lead, plus the slowest round trip and clock
    /// uncertainty.
    pub(crate) fn deadline(&self) -> Instant {
        let core = lock_unpoisoned(&self.shared.core);
        let slowest = core
            .bindings
            .iter()
            .map(|binding| {
                binding.round_trip
                    + Duration::from_micros(u64::from(binding.uncertainty.unwrap_or(0)))
            })
            .max()
            .unwrap_or_default();
        Instant::now()
            + Duration::from_micros(u64::from(donder_device_protocol::SCHEDULE_LEAD_MICROS))
            + slowest
    }

    /// Every controller takes `action` at `position` of its show when the
    /// editor's clock reaches `at`. A controller that cannot is released and
    /// named in the result.
    pub(crate) fn schedule(
        &self,
        at: Instant,
        action: Action,
        position: u32,
        rate: PlaybackRate,
    ) -> Scheduled {
        let mut core = lock_unpoisoned(&self.shared.core);
        let clock_id = core.clock_id;
        let at_micros = at.saturating_duration_since(core.origin).as_micros() as u64;
        let results = thread::scope(|scope| {
            core.bindings
                .iter_mut()
                .map(|binding| {
                    scope.spawn(move || {
                        binding.schedule(clock_id, at_micros, action, position, rate)
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|sent| {
                    sent.join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
                .collect::<Vec<_>>()
        });
        let mut scheduled = Scheduled {
            addresses: Vec::new(),
            failures: DeviceFailures::new(),
        };
        for (binding, result) in std::mem::take(&mut core.bindings).into_iter().zip(results) {
            match result {
                Ok(()) => {
                    scheduled.addresses.push(binding.client.address);
                    core.bindings.push(binding);
                }
                Err(error) => {
                    let _ = binding.client.stop_loaded();
                    scheduled.failures.insert(binding.id, error);
                }
            }
        }
        scheduled
    }
}

fn bind(core: &Core, device: &WantedDevice) -> Result<Binding, String> {
    let client = DeviceClient::new(device.address)?;
    let bind_address = if client.address.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let address = client.address;
    let clock_socket = UdpSocket::bind(bind_address)
        .map_err(|error| format!("Could not open a clock socket for {address}: {error}"))?;
    clock_socket
        .connect(address)
        .map_err(|error| format!("Could not connect the clock socket to {address}: {error}"))?;
    clock_socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .and_then(|()| clock_socket.set_write_timeout(Some(Duration::from_secs(2))))
        .map_err(|error| format!("Could not set the clock timeout for {address}: {error}"))?;
    // Changing clock masters while a show runs would move its time origin.
    let stopped = client.transport(true)?;
    let mut binding = Binding {
        id: device.id.clone(),
        client,
        clock_socket,
        ports: device.ports.clone(),
        prepared: None,
        held: None,
        sample: None,
        rate_ppb: 0,
        uncertainty: None,
        round_trip: Duration::ZERO,
        command_id: stopped.playback.map_or(0, |playback| playback.command_id),
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
    fn udp_clock_exchange_echoes_send_time() {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let mut request = [0; clock::Request::BYTES + 1];
            let (length, peer) = server.recv_from(&mut request).unwrap();
            let request = clock::Request::decode(&request[..length]).unwrap();
            let reply = clock::Reply {
                sent_micros: request.sent_micros,
                boot_id: 7,
                received_micros: 600000,
                replied_micros: 600000,
            }
            .encode();
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
        let sample = udp_clock_sample(&socket, Instant::now()).unwrap();
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
