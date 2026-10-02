use super::{DesktopState, lock_unpoisoned};
use crate::dto::{
    AppSnapshot, AudioTransportState, DevicePlaybackMode, GuiDocumentRequest, SequenceDeviceStatus,
};

impl DesktopState {
    pub(crate) fn connect_sequence_device(
        &self,
        request: &GuiDocumentRequest,
        selected: &[u32],
        address: &str,
        token: &str,
    ) -> Result<Vec<SequenceDeviceStatus>, String> {
        let _operation = lock_unpoisoned(&self.transport_operation);
        if matches!(self.audio_snapshot().state, AudioTransportState::Playing) {
            return Err("Pause the sequence editor before connecting another device".into());
        }
        let (session, _) = self.sequence_export_session(request)?;
        if selected.is_empty() {
            return Err("Select the outputs to map to this device".into());
        }
        let available = super::sequence_export::outputs(&session)?;
        let mut unique = std::collections::BTreeSet::new();
        let ports = selected
            .iter()
            .map(|index| {
                if !unique.insert(*index) {
                    return Err("An output was selected more than once".to_string());
                }
                available
                    .get(*index as usize)
                    .map(|(controller, port, _)| (controller.clone(), *port))
                    .ok_or_else(|| "Selected output is unavailable".into())
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.device_playback
            .register(self.snapshot().project_epoch, address, token, ports)
    }
    pub(crate) fn disconnect_sequence_device(
        &self,
        address: &str,
    ) -> Result<Vec<SequenceDeviceStatus>, String> {
        let _operation = lock_unpoisoned(&self.transport_operation);
        self.device_playback.unregister(address)
    }
    pub(crate) fn sequence_devices(&self) -> Vec<SequenceDeviceStatus> {
        self.device_playback.statuses()
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
        let duration = self.prepare_device_sequence()?;
        let audio = self.audio_snapshot();
        let position = if matches!(audio.state, AudioTransportState::Ended) {
            audio.home_seconds
        } else {
            audio.position_seconds
        };
        let schedule = self.device_playback.schedule(
            DevicePlaybackMode::Playing,
            seconds_to_micros(position)?,
            false,
            duration,
            std::time::Instant::now(),
        )?;
        let result = lock_unpoisoned(&self.audio).play_at(
            schedule.deadline,
            schedule.position as f32 / 1_000_000.0,
            duration as f32 / 1_000_000.0,
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

    pub(super) fn device_audio_hold(
        &self,
        mode: DevicePlaybackMode,
        position: Option<f32>,
        set_home: bool,
    ) -> Result<AppSnapshot, String> {
        let _operation = lock_unpoisoned(&self.transport_operation);
        let audio = self.audio_snapshot();
        let observed_at = std::time::Instant::now();
        let duration = if matches!(audio.state, AudioTransportState::Playing) {
            seconds_to_micros(audio.duration_seconds)?
        } else {
            self.prepare_device_sequence()?
        };
        let position = position.unwrap_or(audio.position_seconds);
        let schedule = self.device_playback.schedule(
            mode,
            seconds_to_micros(position)?,
            position == audio.position_seconds
                && matches!(audio.state, AudioTransportState::Playing)
                && matches!(mode, DevicePlaybackMode::Paused)
                && !set_home,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceClient;
    use std::time::Duration;

    #[test]
    #[ignore = "requires the commissioned controller and session credentials"]
    fn live_controller_follows_editor_transport() {
        let address =
            std::env::var("DONDER_TEST_DEVICE_ADDRESS").expect("Device address is required");
        let token = std::env::var("DONDER_TEST_DEVICE_TOKEN").expect("Device token is required");
        let project =
            std::env::var("DONDER_TEST_PROJECT").expect("Commissioning project is required");
        let client = DeviceClient::new(&address, &token).unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(&project);
        let session = state.project_session().unwrap();
        let sequence = session.project.root().sequences[0].id();
        let request = state
            .resolve_gui_source(
                &sequence.0.module_id().to_string(),
                sequence.0.document().as_str(),
                sequence.0.root_source().object(),
            )
            .unwrap();
        state.load_sequence_audio(request.clone());
        state.render_refresh.finish_pending();
        let ports = state
            .sequence_export_ports(&request)
            .unwrap()
            .into_iter()
            .map(|port| port.index)
            .collect::<Vec<_>>();
        state
            .connect_sequence_device(&request, &ports, &address, &token)
            .unwrap();
        let started = state.audio_play();
        assert_eq!(
            started.audio_transport.state,
            AudioTransportState::Playing,
            "{}",
            started.status
        );
        assert!(started.audio_transport.start_delay_seconds > 0.0);
        std::thread::sleep(Duration::from_millis(600));
        let playing = client.transport(None).unwrap().playback.unwrap();
        assert!(matches!(playing.mode, DevicePlaybackMode::Playing));
        assert!(playing.position_micros > 0);
        let paused = state.audio_pause();
        assert_eq!(
            paused.audio_transport.state,
            AudioTransportState::Paused,
            "{}",
            paused.status
        );
        let held = client.transport(None).unwrap().playback.unwrap();
        assert!(matches!(held.mode, DevicePlaybackMode::Paused));
        assert!(
            held.position_micros
                .abs_diff(seconds_to_micros(paused.audio_transport.position_seconds).unwrap())
                <= 2
        );
        let sought = state.audio_seek(2.0);
        assert_eq!(
            sought.audio_transport.position_seconds, 2.0,
            "{}",
            sought.status
        );
        assert_eq!(
            client
                .transport(None)
                .unwrap()
                .playback
                .unwrap()
                .position_micros,
            2_000_000
        );
        let stopped = state.audio_stop();
        assert_eq!(
            stopped.audio_transport.state,
            AudioTransportState::Stopped,
            "{}",
            stopped.status
        );
        let before = client.transport(None).unwrap().playback.unwrap();
        assert!(matches!(before.mode, DevicePlaybackMode::Stopped));
        let resumed = state.audio_play();
        assert_eq!(
            resumed.audio_transport.state,
            AudioTransportState::Playing,
            "{}",
            resumed.status
        );
        std::thread::sleep(Duration::from_millis(400));
        let after = client.transport(None).unwrap().playback.unwrap();
        assert!(
            after.command_id > before.command_id,
            "Unchanged replay must reuse the uploaded archive and command counter"
        );
        assert_eq!(after.archive_crc, before.archive_crc);
        assert_eq!(after.archive_bytes, before.archive_bytes);
        state.audio_stop();
        state.disconnect_sequence_device(&address).unwrap();
    }
}
