use super::{DesktopState, lock_unpoisoned};
use crate::audio::DeviceBoundary;
use crate::device::DeviceClient;
use crate::device::playback::{DevicePorts, WantedDevice};
use crate::dto::{
    AppSnapshot, AudioTransportState, DevicePlaybackMode, DonderDeviceClaim,
    DonderDeviceConnection, DonderDeviceNetworkRequest, DonderDeviceStatus,
};
use donder_language::controller::{ControllerPortAddress, ControllerProtocol, DonderDeviceId};
use donder_runtime::PlaybackRate;

impl DesktopState {
    pub(super) fn schedule_device_reconcile(&self) {
        self.device_reconcile.schedule(());
    }

    /// Connect editor playback to every Donder controller in the open setup
    /// that is on the network and claimed by this computer, then publish the
    /// device list. Performs network I/O on the reconcile task.
    pub(super) fn reconcile_devices(&self) {
        let _operation = lock_unpoisoned(&self.transport_operation);
        // New bindings stop the controller's show; never interrupt a running one.
        if !matches!(self.audio_snapshot().state, AudioTransportState::Playing) {
            self.connect_setup_devices();
        }
        self.publish_devices();
    }

    pub(super) fn has_setup_devices(&self) -> bool {
        !self.setup_devices().is_empty()
    }

    /// Play drives every Donder controller in the open setup. Connect any that
    /// are not yet connected, and name each one that still cannot play.
    /// The caller holds the transport operation lock.
    fn require_setup_devices(&self) -> Result<(), String> {
        self.connect_setup_devices();
        self.publish_devices();
        let devices = self.snapshot().devices;
        let problems = self
            .setup_devices()
            .into_iter()
            .filter_map(
                |(id, _)| match devices.iter().find(|device| device.id == id.as_str()) {
                    None => Some(format!("controller {} is not on the network", id.as_str())),
                    Some(device) => match &device.connection {
                        DonderDeviceConnection::Connected { .. } => None,
                        DonderDeviceConnection::Failed { error } => {
                            Some(format!("{}: {error}", device.name))
                        }
                        DonderDeviceConnection::Unused => {
                            Some(format!("{} is not connected", device.name))
                        }
                    },
                },
            )
            .collect::<Vec<_>>();
        if problems.is_empty() {
            Ok(())
        } else {
            Err(format!("Cannot play on {}", problems.join("; ")))
        }
    }

    /// The caller holds the transport operation lock.
    fn connect_setup_devices(&self) {
        {
            let advertised = self.discovery.devices();
            let tokens = self.persistence.device_tokens();
            let wanted = self
                .setup_devices()
                .into_iter()
                .filter_map(|(id, ports)| {
                    let device = advertised.get(&id)?;
                    let token = tokens.get(id.as_str())?.clone();
                    (device.format == donder_runtime::FORMAT_VERSION).then_some(WantedDevice {
                        id,
                        address: device.address,
                        token,
                        ports,
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
    }

    pub(super) fn publish_devices(&self) {
        let advertised = self.discovery.devices();
        let tokens = self.persistence.device_tokens();
        let used = self
            .setup_devices()
            .into_iter()
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        let connections = self.device_playback.connections();
        let failures = lock_unpoisoned(&self.device_failures).clone();
        let devices = advertised
            .into_iter()
            .map(|(id, device)| {
                let claimed_here = tokens.contains_key(id.as_str());
                let firmware_current = device.format == donder_runtime::FORMAT_VERSION;
                let connection = match (connections.get(&id), failures.get(&id)) {
                    (Some(Ok(uncertainty)), _) => DonderDeviceConnection::Connected {
                        clock_uncertainty_micros: *uncertainty,
                    },
                    (Some(Err(error)), _) | (None, Some(error)) => DonderDeviceConnection::Failed {
                        error: error.clone(),
                    },
                    (None, None) if !used.contains(&id) => DonderDeviceConnection::Unused,
                    (None, None) if !claimed_here => DonderDeviceConnection::Failed {
                        error: "Claim this controller to use it for playback.".into(),
                    },
                    (None, None) if !firmware_current => DonderDeviceConnection::Failed {
                        error: "Controller firmware does not match this editor. Install the bundled firmware over USB.".into(),
                    },
                    (None, None) => DonderDeviceConnection::Failed {
                        error: "Connecting...".into(),
                    },
                };
                DonderDeviceStatus {
                    claim: match (device.claimed, claimed_here) {
                        (false, _) => DonderDeviceClaim::Unclaimed,
                        (true, true) => DonderDeviceClaim::Claimed,
                        (true, false) => DonderDeviceClaim::ClaimedElsewhere,
                    },
                    id: id.as_str().to_string(),
                    name: device.name,
                    address: device.address.to_string(),
                    network: device.network,
                    firmware_current,
                    connection,
                }
            })
            .collect::<Vec<_>>();
        let error = self.discovery.error().map(str::to_string);
        if self.snapshot().devices != devices || error.is_some() {
            self.update_snapshot(|snapshot| {
                snapshot.devices = devices;
                if let Some(error) = error {
                    snapshot.status = error;
                }
            });
        }
    }

    /// Donder controllers in the open setup with their ports in output order.
    fn setup_devices(&self) -> Vec<(DonderDeviceId, DevicePorts)> {
        let Some(session) = self.project_session() else {
            return Vec::new();
        };
        let project = &session.project;
        let Some(setup) = project.setup(project.root().setup.id()) else {
            return Vec::new();
        };
        setup
            .controllers
            .iter()
            .filter_map(|source| project.controller(source.id()))
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
                Some((
                    config.device.clone(),
                    ports
                        .into_iter()
                        .map(|(_, port)| (controller.id.clone(), port))
                        .collect(),
                ))
            })
            .collect()
    }

    fn device_client(&self, id: &str) -> Result<DeviceClient, String> {
        let id = DonderDeviceId::parse(id).ok_or("Invalid controller ID.")?;
        let device = self
            .discovery
            .devices()
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
            .discovery
            .devices()
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
            DevicePlaybackMode::Playing
        } else {
            DevicePlaybackMode::Stopped
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
        let (_, sequence) = lock_unpoisoned(&self.workspace)
            .render_target
            .clone()
            .ok_or("Open a sequence before starting devices")?;
        let duration = donder_language::values::sample_duration_from_donder_duration(
            &session
                .project
                .sequence(&sequence)
                .ok_or("Sequence is missing")?
                .duration,
        )
        .map_err(|error| format!("Invalid sequence duration: {error:?}"))?
        .as_ticks();
        self.device_playback.prepare(
            snapshot.project_epoch,
            snapshot.project_revision,
            &sequence,
            |ports| {
                let available = super::sequence_export::outputs(&session)?;
                let widths = ports
                    .iter()
                    .map(|(controller, port)| {
                        available
                            .iter()
                            .find(|(id, output, _)| id == controller && output == port)
                            .map(|(_, _, width)| *width)
                            .ok_or("A connected device output was removed from the setup")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let prepared = donder_elaboration::prepare(
                    &session.project,
                    &sequence,
                    donder_elaboration::PrepareOutputs::Ports(ports),
                )
                .ok_or("The sequence or a connected device output is unavailable.")?;
                Ok((
                    donder_runtime::encode_sequence(&prepared)
                        .map_err(|error| format!("Could not encode device sequence: {error:?}"))?,
                    widths,
                ))
            },
        )?;
        Ok(duration)
    }

    pub(super) fn device_audio_play(&self) -> Result<AppSnapshot, String> {
        let _operation = lock_unpoisoned(&self.transport_operation);
        if matches!(self.audio_snapshot().state, AudioTransportState::Playing) {
            return Ok(self.snapshot());
        }
        self.require_setup_devices()?;
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
        position: f32,
        advancing: Option<PlaybackRate>,
        rate: PlaybackRate,
        duration: u32,
    ) -> Result<AppSnapshot, String> {
        let schedule = self.device_playback.schedule(
            DevicePlaybackMode::Playing,
            seconds_to_micros(position)?,
            advancing,
            rate,
            duration,
            std::time::Instant::now(),
        )?;
        let result = lock_unpoisoned(&self.audio).play_at(
            schedule.deadline,
            schedule.position as f32 / 1_000_000.0,
            duration as f32 / 1_000_000.0,
            rate,
        );
        match result {
            Ok(transport) => Ok(self.update_snapshot(|snapshot| {
                snapshot.audio_transport = transport;
                snapshot.status = "Sequence scheduled on connected devices.".into();
            })),
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
                self.device_audio_hold_locked(DevicePlaybackMode::Stopped, Some(home), false)?;
            }
        }
        Ok(())
    }

    pub(super) fn device_audio_hold(
        &self,
        mode: DevicePlaybackMode,
        position: Option<f32>,
        set_home: bool,
    ) -> Result<AppSnapshot, String> {
        let _operation = lock_unpoisoned(&self.transport_operation);
        self.device_audio_hold_locked(mode, position, set_home)
    }

    /// The caller holds the transport operation lock.
    fn device_audio_hold_locked(
        &self,
        mode: DevicePlaybackMode,
        position: Option<f32>,
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
        let schedule = self.device_playback.schedule(
            mode,
            seconds_to_micros(position)?,
            (position == audio.position_seconds
                && matches!(audio.state, AudioTransportState::Playing)
                && matches!(mode, DevicePlaybackMode::Paused)
                && !set_home)
                .then_some(rate),
            rate,
            duration,
            observed_at,
        )?;
        let state = if matches!(mode, DevicePlaybackMode::Stopped) {
            AudioTransportState::Stopped
        } else {
            AudioTransportState::Paused
        };
        lock_unpoisoned(&self.audio).hold_at(
            schedule.deadline,
            state,
            schedule.position as f32 / 1_000_000.0,
            set_home,
        );
        // The audio backend has its pause scheduled already. Publish the final seek/hold
        // snapshot at T, including when the previous state was already paused.
        std::thread::sleep(
            schedule
                .deadline
                .saturating_duration_since(std::time::Instant::now()),
        );
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

fn seconds_to_micros(seconds: f32) -> Result<u32, String> {
    donder_language::values::sample_time_from_seconds_f32(seconds)
        .map(|time| time.as_ticks())
        .map_err(|error| format!("Invalid playback position: {error:?}"))
}
