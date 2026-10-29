use super::{DesktopState, lock_unpoisoned};
use crate::audio::DeviceBoundary;
use crate::device::DeviceClient;
use crate::device::discovery::{Advertised, Advertisements, DeviceDiscovery};
use crate::device::playback::{DeviceFailures, DevicePorts, WantedDevice};
use donder_device_protocol::Mode;
use donder_editor::dto::{
    AppSnapshot, AudioTransportState, DonderDeviceClaim, DonderDeviceConnection,
    DonderDeviceNetworkRequest, DonderDeviceStatus, LiveOutputControllerSnapshot,
    LiveOutputControllerState, LiveOutputSnapshot, LiveOutputState,
};
use donder_model::{ControllerId, ControllerPortAddress, ControllerProtocol, DonderDeviceId};
use donder_runtime::PlaybackRate;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

/// A Donder controller in the open setup.
struct SetupDevice {
    controller: ControllerId,
    device: DonderDeviceId,
    /// Its ports in output order.
    ports: DevicePorts,
}

/// What the editor knows of its controllers' playback links.
struct DeviceLinks {
    live: bool,
    advertised: Advertisements,
    tokens: BTreeMap<String, String>,
    connections: BTreeMap<DonderDeviceId, Result<Option<u32>, String>>,
    failures: DeviceFailures,
}

impl DeviceLinks {
    /// The link to `id`, which the open setup uses when `used`.
    fn connection(&self, id: &DonderDeviceId, used: bool) -> DonderDeviceConnection {
        if !used {
            return DonderDeviceConnection::Unused;
        }
        if self.live {
            match (self.connections.get(id), self.failures.get(id)) {
                (Some(Ok(uncertainty)), _) => {
                    return DonderDeviceConnection::Connected {
                        clock_uncertainty_micros: *uncertainty,
                    };
                }
                (Some(Err(error)), _) | (None, Some(error)) => {
                    return DonderDeviceConnection::Failed {
                        error: error.clone(),
                    };
                }
                (None, None) => {}
            }
        }
        let failed = |error: &str| DonderDeviceConnection::Failed {
            error: error.into(),
        };
        match self.advertised.get(id) {
            None if self.live => failed("Not on the network."),
            None => DonderDeviceConnection::Unused,
            Some(_) if !self.tokens.contains_key(id.as_str()) => {
                failed("Claim this controller to use it for playback.")
            }
            Some(Advertised { format, .. }) if *format != donder_runtime::FORMAT_VERSION => failed(
                "Controller firmware does not match this editor. Flash the current firmware (see firmware/esp32/README.md).",
            ),
            Some(_) if self.live => DonderDeviceConnection::Connecting,
            Some(_) => DonderDeviceConnection::Unused,
        }
    }
}

impl DesktopState {
    pub(super) fn schedule_device_reconcile(&self) {
        self.device_reconcile.schedule(());
    }

    pub(super) fn devices_live(&self) -> bool {
        self.devices_live.load(Ordering::Acquire)
    }

    /// Live output drives the setup's Donder controllers, or releases them.
    pub(super) fn set_devices_live(&self, live: bool) {
        if self.devices_live.swap(live, Ordering::AcqRel) != live {
            self.update_discovery();
            self.schedule_device_reconcile();
        }
    }

    /// A view that lists Donder controllers opened or closed.
    pub(crate) fn watch_devices(&self, watching: bool) {
        {
            let mut watchers = lock_unpoisoned(&self.device_watchers);
            *watchers = if watching {
                watchers.saturating_add(1)
            } else {
                watchers.saturating_sub(1)
            };
        }
        self.update_discovery();
        self.schedule_device_reconcile();
    }

    /// Discovery runs while live output drives Donder controllers or a view
    /// lists them; dropping it shuts its daemon down.
    fn update_discovery(&self) {
        let wanted = self.devices_live() || *lock_unpoisoned(&self.device_watchers) > 0;
        let stopped = {
            let mut discovery = lock_unpoisoned(&self.discovery);
            if !wanted {
                discovery.take()
            } else {
                if discovery.is_none() {
                    let weak = self.weak.clone();
                    *discovery = Some(DeviceDiscovery::new(move || {
                        if let Some(inner) = weak.upgrade() {
                            DesktopState(inner).schedule_device_reconcile();
                        }
                    }));
                }
                None
            }
        };
        drop(stopped);
    }

    fn advertised(&self) -> Advertisements {
        lock_unpoisoned(&self.discovery)
            .as_ref()
            .map(DeviceDiscovery::devices)
            .unwrap_or_default()
    }

    /// Connect live output to every Donder controller in the open setup that
    /// is on the network and claimed by this computer, or release them all
    /// while live output is off, then publish the device list. Performs
    /// network I/O on the reconcile task.
    pub(super) fn reconcile_devices(&self) {
        let _operation = lock_unpoisoned(&self.transport_operation);
        if !self.devices_live() {
            // Releasing a binding stops the show it plays.
            self.device_playback
                .sync(self.snapshot().project_epoch, Vec::new());
            lock_unpoisoned(&self.device_failures).clear();
        } else if !matches!(self.audio_snapshot().state, AudioTransportState::Playing) {
            // New bindings stop the controller's show; never interrupt a running one.
            self.connect_setup_devices();
        }
        self.publish_devices();
    }

    /// The caller holds the transport operation lock.
    fn connect_setup_devices(&self) {
        let advertised = self.advertised();
        let tokens = self.persistence.device_tokens();
        let wanted = self
            .setup_devices()
            .into_iter()
            .filter_map(|setup| {
                let device = advertised.get(&setup.device)?;
                let token = tokens.get(setup.device.as_str())?.clone();
                (device.format == donder_runtime::FORMAT_VERSION).then_some(WantedDevice {
                    id: setup.device,
                    address: device.address,
                    token,
                    ports: setup.ports,
                })
            })
            .collect();
        let failures = self
            .device_playback
            .sync(self.snapshot().project_epoch, wanted)
            .into_iter()
            .filter_map(|(id, result)| result.err().map(|error| (id, error)))
            .collect();
        *lock_unpoisoned(&self.device_failures) = failures;
    }

    fn record_device_failures(&self, failures: DeviceFailures) {
        lock_unpoisoned(&self.device_failures).extend(failures);
    }

    fn device_links(&self) -> DeviceLinks {
        DeviceLinks {
            live: self.devices_live(),
            advertised: self.advertised(),
            tokens: self.persistence.device_tokens(),
            connections: self.device_playback.connections(),
            failures: lock_unpoisoned(&self.device_failures).clone(),
        }
    }

    /// Live output's view: its streamed controllers, then, while live output
    /// drives them, each Donder controller in the open setup.
    pub(super) fn live_output_view(&self, mut output: LiveOutputSnapshot) -> LiveOutputSnapshot {
        if !self.devices_live() {
            return output;
        }
        let links = self.device_links();
        for setup in self.setup_devices() {
            let (state, last_error) = match links.connection(&setup.device, true) {
                DonderDeviceConnection::Connected { .. } => {
                    (LiveOutputControllerState::Active, None)
                }
                DonderDeviceConnection::Failed { error } => (
                    LiveOutputControllerState::Error,
                    Some(format!(
                        "Donder controller {}: {error}",
                        setup.device.as_str()
                    )),
                ),
                DonderDeviceConnection::Connecting | DonderDeviceConnection::Unused => {
                    (LiveOutputControllerState::Opening, None)
                }
            };
            if matches!(state, LiveOutputControllerState::Active) {
                output.active_controller_count += 1;
            }
            output.controllers.push(LiveOutputControllerSnapshot {
                id: format!("{:?}", setup.controller.0),
                state,
                last_error,
            });
        }
        // Streamed output that stopped or failed leaves the Donder controllers live.
        if matches!(
            output.state,
            LiveOutputState::Disabled | LiveOutputState::Error
        ) {
            output.state = if matches!(self.audio_snapshot().state, AudioTransportState::Playing) {
                LiveOutputState::Streaming
            } else {
                LiveOutputState::Holding
            };
        }
        output
    }

    pub(super) fn publish_devices(&self) {
        let links = self.device_links();
        let used = self
            .setup_devices()
            .into_iter()
            .map(|setup| setup.device)
            .collect::<Vec<_>>();
        let devices = links
            .advertised
            .iter()
            .map(|(id, device)| DonderDeviceStatus {
                claim: match (device.claimed, links.tokens.contains_key(id.as_str())) {
                    (false, _) => DonderDeviceClaim::Unclaimed,
                    (true, true) => DonderDeviceClaim::Claimed,
                    (true, false) => DonderDeviceClaim::ClaimedElsewhere,
                },
                id: id.as_str().to_string(),
                name: device.name.clone(),
                address: device.address.to_string(),
                network: device.network,
                firmware_current: device.format == donder_runtime::FORMAT_VERSION,
                connection: links.connection(id, used.contains(id)),
            })
            .collect::<Vec<_>>();
        let live_output = self.live_output_view(lock_unpoisoned(&self.live_output).snapshot());
        let error = lock_unpoisoned(&self.discovery)
            .as_ref()
            .and_then(|discovery| discovery.error().map(str::to_string));
        let snapshot = self.snapshot();
        if snapshot.devices != devices || snapshot.live_output != live_output || error.is_some() {
            self.update_snapshot(|snapshot| {
                snapshot.devices = devices;
                snapshot.live_output = live_output;
                if let Some(error) = error {
                    snapshot.status = error;
                }
            });
        }
    }

    /// Donder controllers in the open setup.
    fn setup_devices(&self) -> Vec<SetupDevice> {
        let Some(session) = self.project_session() else {
            return Vec::new();
        };
        let project = &session.project;
        let Some(setup) = project.get::<donder_model::Setup>(*project.root().setup.id()) else {
            return Vec::new();
        };
        setup
            .controllers
            .iter()
            .filter_map(|source| project.get::<donder_model::Controller>(*source.id()))
            .filter_map(|controller| {
                let ControllerProtocol::Donder(config) = &controller.protocol else {
                    return None;
                };
                let mut ports = controller
                    .ports
                    .iter()
                    .filter_map(|port| match port.address {
                        ControllerPortAddress::DonderOutput(output) => Some((output, port.id)),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                ports.sort();
                Some(SetupDevice {
                    controller: controller.id,
                    device: config.device.clone(),
                    ports: ports
                        .into_iter()
                        .map(|(_, port)| (controller.id, port))
                        .collect(),
                })
            })
            .collect()
    }

    fn device_client(&self, id: &str) -> Result<DeviceClient, String> {
        let id = DonderDeviceId::parse(id).ok_or("Invalid controller ID.")?;
        let device = self
            .advertised()
            .remove(&id)
            .ok_or("Controller is not on the network.")?;
        let token = self
            .persistence
            .device_tokens()
            .remove(id.as_str())
            .ok_or("Claim this controller first.")?;
        DeviceClient::new(device.address, &token)
    }

    pub(crate) fn claim_device(&self, id: &str) -> Result<AppSnapshot, String> {
        let device_id = DonderDeviceId::parse(id).ok_or("Invalid controller ID.")?;
        let device = self
            .advertised()
            .remove(&device_id)
            .ok_or("Controller is not on the network.")?;
        let token = crate::device::claim(device.address)?;
        self.persistence.record_device_token(id, token)?;
        self.schedule_device_reconcile();
        self.publish_devices();
        Ok(self.snapshot())
    }

    pub(crate) fn rename_device(&self, id: &str, name: &str) -> Result<AppSnapshot, String> {
        self.device_client(id)?.rename(name)?;
        Ok(self.snapshot())
    }

    /// Loop the controller's saved show without the editor, or stop it.
    pub(crate) fn set_device_standalone(
        &self,
        id: &str,
        playing: bool,
    ) -> Result<AppSnapshot, String> {
        let client = self.device_client(id)?;
        if client.transport(None)?.playback.is_none() {
            return Err(
                "The controller has no saved show. Press Play in a sequence to upload one.".into(),
            );
        }
        client.transport(Some(if playing {
            Mode::Playing
        } else {
            Mode::Stopped
        }))?;
        Ok(self.update_snapshot(|snapshot| {
            snapshot.status = if playing {
                "Controller is looping its saved show.".into()
            } else {
                "Controller stopped its saved show.".into()
            }
        }))
    }

    pub(crate) fn set_device_network(
        &self,
        id: &str,
        network: Option<DonderDeviceNetworkRequest>,
    ) -> Result<AppSnapshot, String> {
        self.device_client(id)?.set_network(network.as_ref())?;
        Ok(self.update_snapshot(|snapshot| {
            snapshot.status = match network {
                Some(network) => format!(
                    "Controller is restarting to join {}. Connect this computer to that network.",
                    network.ssid
                ),
                None => "Controller is restarting with its own access point.".into(),
            }
        }))
    }

    fn prepare_device_sequence(&self) -> Result<u32, String> {
        let _authoring = lock_unpoisoned(&self.authoring);
        let snapshot = self.snapshot();
        let session = self
            .project_session()
            .ok_or("Open a valid project before starting devices")?;
        let target = lock_unpoisoned(&self.workspace)
            .render_target
            .clone()
            .ok_or("Open a sequence before starting devices")?;
        let (_, sequence) = target.resolve(&session).ok_or("Sequence is missing")?;
        let model = session
            .project
            .get::<donder_model::Sequence>(sequence)
            .ok_or("Sequence is missing")?;
        let duration = donder_language::sample_duration_from_donder_duration(&model.duration)
            .map_err(|error| format!("Invalid sequence duration: {error:?}"))?
            .as_ticks();
        let failures = self.device_playback.prepare(
            snapshot.project_epoch,
            snapshot.project_revision,
            &target.sequence,
            |ports| {
                donder_elaboration::prepare(
                    &session.project,
                    &sequence,
                    donder_elaboration::Retain::Ports(ports),
                )
                .ok_or_else(|| "The sequence or a connected device output is unavailable.".into())
            },
        )?;
        self.record_device_failures(failures);
        Ok(duration)
    }

    /// Play connects the setup's Donder controllers that are not yet
    /// connected; the editor plays alone when none is reachable.
    pub(super) fn device_audio_play(&self) -> Result<AppSnapshot, String> {
        let _operation = lock_unpoisoned(&self.transport_operation);
        if matches!(self.audio_snapshot().state, AudioTransportState::Playing) {
            return Ok(self.snapshot());
        }
        self.connect_setup_devices();
        if !self.device_playback.has_devices() {
            self.publish_devices();
            return Ok(self.local_audio_play());
        }
        let duration = self.prepare_device_sequence()?;
        let position = lock_unpoisoned(&self.audio).play_position();
        let rate = lock_unpoisoned(&self.audio).playback_rate();
        self.device_audio_schedule_playing(position, None, rate, duration)
    }

    /// Playing devices restart together from the position reached at the shared deadline.
    pub(super) fn device_audio_set_rate(&self, rate: PlaybackRate) -> Result<AppSnapshot, String> {
        let _operation = lock_unpoisoned(&self.transport_operation);
        let audio = self.audio_snapshot();
        if !matches!(audio.state, AudioTransportState::Playing) {
            let transport = lock_unpoisoned(&self.audio).set_playback_rate(rate);
            return Ok(self.update_snapshot(|snapshot| snapshot.audio_transport = transport));
        }
        let current = lock_unpoisoned(&self.audio).playback_rate();
        self.device_audio_schedule_playing(
            audio.position_seconds,
            Some(current),
            rate,
            seconds_to_micros(audio.duration_seconds)?,
        )
    }

    /// The caller holds the transport operation lock.
    fn device_audio_schedule_playing(
        &self,
        position: f64,
        advancing: Option<PlaybackRate>,
        rate: PlaybackRate,
        duration: u32,
    ) -> Result<AppSnapshot, String> {
        let mut schedule = self.device_playback.schedule(
            Mode::Playing,
            seconds_to_micros(position)?,
            advancing,
            rate,
            duration,
            std::time::Instant::now(),
        )?;
        self.record_device_failures(std::mem::take(&mut schedule.failures));
        let result = lock_unpoisoned(&self.audio).play_at(
            schedule.deadline,
            f64::from(schedule.position) / 1_000_000.0,
            f64::from(duration) / 1_000_000.0,
            rate,
        );
        match result {
            Ok(transport) => {
                self.publish_devices();
                let devices = schedule.device_count();
                Ok(self.update_snapshot(|snapshot| {
                    snapshot.audio_transport = transport;
                    if devices > 0 {
                        snapshot.status =
                            format!("Sequence scheduled on {devices} Donder controllers.");
                    }
                }))
            }
            Err(error) => Err(format!(
                "Could not schedule audio: {error}. {}",
                self.device_playback.cancel(&schedule)
            )),
        }
    }

    /// Devices play straight through, so the shared timeline restarts at the loop start or holds
    /// at Home where local playback would wrap or stop.
    pub(super) fn device_audio_cross_boundary(&self) -> Result<(), String> {
        let _operation = lock_unpoisoned(&self.transport_operation);
        // A transport command may have moved the timeline since the poll saw the boundary.
        let Some(boundary) = lock_unpoisoned(&self.audio).device_boundary() else {
            return Ok(());
        };
        match boundary {
            DeviceBoundary::Wrap(start) => {
                let duration = seconds_to_micros(self.audio_snapshot().duration_seconds)?;
                let rate = lock_unpoisoned(&self.audio).playback_rate();
                self.device_audio_schedule_playing(start, None, rate, duration)?;
            }
            DeviceBoundary::Stop(home) => {
                self.device_audio_hold_locked(Mode::Stopped, Some(home), false)?;
            }
        }
        Ok(())
    }

    pub(super) fn device_audio_hold(
        &self,
        mode: Mode,
        position: Option<f64>,
        set_home: bool,
    ) -> Result<AppSnapshot, String> {
        let _operation = lock_unpoisoned(&self.transport_operation);
        self.device_audio_hold_locked(mode, position, set_home)
    }

    /// The caller holds the transport operation lock.
    fn device_audio_hold_locked(
        &self,
        mode: Mode,
        position: Option<f64>,
        set_home: bool,
    ) -> Result<AppSnapshot, String> {
        let audio = self.audio_snapshot();
        let observed_at = std::time::Instant::now();
        let duration = if matches!(audio.state, AudioTransportState::Playing) {
            seconds_to_micros(audio.duration_seconds)?
        } else {
            self.prepare_device_sequence()?
        };
        let position = position.unwrap_or(audio.position_seconds);
        let rate = lock_unpoisoned(&self.audio).playback_rate();
        let mut schedule = self.device_playback.schedule(
            mode,
            seconds_to_micros(position)?,
            (position == audio.position_seconds
                && matches!(audio.state, AudioTransportState::Playing)
                && matches!(mode, Mode::Paused)
                && !set_home)
                .then_some(rate),
            rate,
            duration,
            observed_at,
        )?;
        self.record_device_failures(std::mem::take(&mut schedule.failures));
        let state = if matches!(mode, Mode::Stopped) {
            AudioTransportState::Stopped
        } else {
            AudioTransportState::Paused
        };
        lock_unpoisoned(&self.audio).hold_at(
            schedule.deadline,
            state,
            f64::from(schedule.position) / 1_000_000.0,
            set_home,
        );
        // The audio backend has its pause scheduled already. Publish the final seek/hold
        // snapshot at T, including when the previous state was already paused.
        std::thread::sleep(
            schedule
                .deadline
                .saturating_duration_since(std::time::Instant::now()),
        );
        self.publish_devices();
        let transport = self.audio_snapshot();
        Ok(self.update_snapshot(|snapshot| snapshot.audio_transport = transport))
    }
    pub(super) fn device_transport_error(&self, error: String) -> AppSnapshot {
        let transport = self.audio_snapshot();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = transport;
            snapshot.status = format!("Device playback failed: {error}")
        })
    }
}

fn seconds_to_micros(seconds: f64) -> Result<u32, String> {
    donder_language::sample_time_from_seconds_f64(seconds)
        .map(|time| time.as_ticks())
        .map_err(|error| format!("Invalid playback position: {error:?}"))
}
