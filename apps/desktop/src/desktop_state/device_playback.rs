use super::{DesktopState, lock_unpoisoned};
use crate::audio::{Timeline, WindowEnd};
use crate::device::DeviceClient;
use crate::device::discovery::{Advertised, Advertisements, DeviceDiscovery};
use crate::device::playback::{DeviceFailures, DevicePorts, WantedDevice};
use donder_device_protocol::Action;
use donder_editor::dto::{
    AppSnapshot, AudioTransportState, DonderDeviceConnection, DonderDeviceNetworkRequest,
    DonderDeviceStatus, LiveOutputControllerSnapshot, LiveOutputControllerState,
    LiveOutputSnapshot, LiveOutputState,
};
use donder_model::{ControllerId, ControllerOutput, ControllerProtocol, DonderDeviceId};
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::time::Instant;

/// A Donder controller in the project.
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
    connections: BTreeMap<DonderDeviceId, Result<Option<u32>, String>>,
    failures: DeviceFailures,
}

impl DeviceLinks {
    /// The link to `id`, which the project uses when `used`.
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
            Some(Advertised { format, .. }) if *format != donder_runtime::FORMAT_VERSION => {
                failed("Firmware doesn't match this version of Donder.")
            }
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

    /// Live output drives the project's Donder controllers, or releases them.
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

    /// Bring the Donder controllers in the project in line with the
    /// editor: connect those on the network and have them follow the
    /// performance in progress or the sequence transport, or release them all
    /// while live output is off, then publish the device list. Performs
    /// network I/O on the reconcile task.
    pub(super) fn reconcile_devices(&self) {
        let _operation = lock_unpoisoned(&self.transport_operation);
        *lock_unpoisoned(&self.device_window_end) = None;
        if !self.devices_live() {
            self.console_feed.stop();
            // Releasing a binding stops the show it plays.
            self.device_playback
                .sync(self.snapshot().project_epoch, Vec::new());
            lock_unpoisoned(&self.device_failures).clear();
        } else {
            self.connect_setup_devices();
            if let Err(error) = self.play_on_devices() {
                self.device_transport_error(error);
            }
        }
        self.publish_devices();
    }

    /// Have the controllers play what plays now, on its clock: take up its
    /// place a lead time from now and, on the transport, wrap or stop where it
    /// next reaches its window end. A performance's controllers join its
    /// inputs, and once joined play on by themselves. The caller holds the
    /// transport operation lock.
    fn play_on_devices(&self) -> Result<(), String> {
        let Some(playing) = self.with_playing(|playing| {
            playing.map(|playing| (playing.id, playing.clock, playing.generation))
        }) else {
            self.console_feed.stop();
            return Ok(());
        };
        let (id, clock, generation) = playing;
        let session = matches!(clock, crate::playing::PlayClock::Session(_));
        if !session {
            self.console_feed.stop();
        }
        let joined = |address| self.console_feed.has_joined(generation, address);
        if !self.device_playback.has_devices()
            || (session && self.device_playback.addresses().into_iter().all(joined))
        {
            return Ok(());
        }
        let Some((timeline, rate)) = clock.anchor(&mut lock_unpoisoned(&self.audio)) else {
            return Ok(());
        };
        let action = match timeline {
            Timeline::Playing { .. } => Action::Play,
            Timeline::Paused(_) => Action::Pause,
            Timeline::Stopped(_) => Action::Stop,
            // Controllers end by themselves.
            Timeline::Ended => return Ok(()),
        };
        let duration = self.prepare_device_show(id, generation)?;
        let at = self.device_playback.deadline();
        let position = seconds_to_micros(timeline.position_at(at))?.min(duration);
        let scheduled = self.device_playback.schedule(at, action, position, rate);
        if session {
            for address in scheduled.addresses() {
                if !joined(address) {
                    self.console_feed.join(generation, address);
                }
            }
        }
        self.record_device_failures(scheduled.failures);
        let Some((end_at, end)) = timeline.next_end(at) else {
            return Ok(());
        };
        // Each controller holds one pending command, the start, until `at`.
        std::thread::sleep(at.saturating_duration_since(Instant::now()));
        // An end too close to schedule passes first; the reconcile after it
        // takes up the transport from there.
        if end_at >= self.device_playback.deadline() {
            let (action, position) = match end {
                WindowEnd::Wrap { start, .. } => (Action::Play, start),
                WindowEnd::Stop { home, .. } => (Action::Stop, home),
            };
            let scheduled =
                self.device_playback
                    .schedule(end_at, action, seconds_to_micros(position)?, rate);
            self.record_device_failures(scheduled.failures);
        }
        *lock_unpoisoned(&self.device_window_end) = Some(end_at);
        Ok(())
    }

    /// The caller holds the transport operation lock.
    fn connect_setup_devices(&self) {
        let advertised = self.advertised();
        let wanted = self
            .setup_devices()
            .into_iter()
            .filter_map(|setup| {
                let device = advertised.get(&setup.device)?;
                (device.format == donder_runtime::FORMAT_VERSION).then_some(WantedDevice {
                    id: setup.device,
                    address: device.address,
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
            connections: self.device_playback.connections(),
            failures: lock_unpoisoned(&self.device_failures).clone(),
        }
    }

    /// Live output's view: its streamed controllers, then, while live output
    /// drives them, each Donder controller in the project.
    pub(super) fn live_output_view(&self, mut output: LiveOutputSnapshot) -> LiveOutputSnapshot {
        output.enabled = self.live_output_enabled();
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

    fn publish_devices(&self) {
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

    /// Donder controllers in the project.
    fn setup_devices(&self) -> Vec<SetupDevice> {
        let Some(session) = self.project_session() else {
            return Vec::new();
        };
        let project = session.project();
        project
            .controllers()
            .filter_map(|controller| {
                let ControllerProtocol::Donder(config) = &controller.protocol else {
                    return None;
                };
                let mut ports = config
                    .ports
                    .iter()
                    .map(|port| (port.output, port.port.id))
                    .collect::<Vec<_>>();
                ports.sort();
                Some(SetupDevice {
                    controller: controller.id(),
                    device: config.device.clone(),
                    ports: ports
                        .into_iter()
                        .map(|(_, port)| (controller.id(), ControllerOutput::Port(port)))
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
        DeviceClient::new(device.address)
    }

    pub(crate) fn rename_device(&self, id: &str, name: &str) -> Result<AppSnapshot, String> {
        self.device_client(id)?.rename(name)?;
        Ok(self.snapshot())
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

    /// Prepare show `id`, which plays as `generation`, for the controllers
    /// and return its duration.
    fn prepare_device_show(
        &self,
        id: donder_model::ShowId,
        generation: u32,
    ) -> Result<u32, String> {
        let _authoring = lock_unpoisoned(&self.authoring);
        let session = self
            .project_session()
            .ok_or("Open a valid project before starting devices")?;
        let object = match id {
            donder_model::ShowId::Sequence(id) => donder_model::ObjectId::from(id),
            donder_model::ShowId::Console(id) => id.into(),
        };
        let source = session
            .source_identity(object)
            .ok_or("The show is missing")?;
        let duration = session
            .project()
            .accepted(id)
            .ok_or("The show is missing")?
            .timing()
            .duration()
            .as_ticks();
        let mut cache = self.take_preparation();
        let prepared = self.device_playback.prepare(
            self.snapshot().project_epoch,
            generation,
            &source,
            |ports| {
                crate::playing::prepare_show(
                    session.project(),
                    id,
                    donder_elaboration::Retain::Outputs(ports),
                    &mut cache,
                )
                .ok_or_else(|| "The show or a connected device output is unavailable.".into())
            },
        );
        self.keep_preparation(cache);
        self.record_device_failures(prepared?);
        Ok(duration)
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
        .map_err(|error| format!("Invalid playback position: {error}"))
}
