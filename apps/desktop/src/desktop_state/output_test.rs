use super::{DesktopState, lock_unpoisoned};
use crate::dto::{AppSnapshot, ControllerOutputTest, DocumentViewId, GuiDocumentRequest};
use donder_elaboration::ControllerPortFrame;
use donder_language::controller::ControllerId;

impl DesktopState {
    pub(crate) fn start_output_test(
        &self,
        request: &GuiDocumentRequest,
        test: ControllerOutputTest,
    ) -> Result<AppSnapshot, String> {
        let _authoring = lock_unpoisoned(&self.authoring);
        if request.project_revision != self.snapshot().project_revision {
            return Err(
                "The project changed. Reopen the output test with the current setup.".into(),
            );
        }
        if request.view != DocumentViewId::Controller {
            return Err("Open a controller to test its outputs.".into());
        }
        let session = self
            .project_session()
            .ok_or("Open a valid project to test outputs.")?;
        let resolved = crate::gui::resolve_request(&session, request)?;
        let id = ControllerId(resolved.identity);
        let mut controller = session
            .project
            .controllers
            .get(&id)
            .ok_or("Controller is missing.")?
            .clone();
        let port = controller
            .ports
            .iter()
            .find(|port| port.id.0 == test.port)
            .ok_or("Choose an output port.")?
            .clone();
        let end = usize::from(test.start_slot) + usize::from(test.slot_count);
        if test.slot_count == 0 || end > usize::from(port.slot_count) {
            return Err("Choose a nonempty channel range within this output.".into());
        }
        let mut slots = vec![0; usize::from(port.slot_count)];
        slots[usize::from(test.start_slot)..end].fill(test.value);
        controller.ports = vec![port.clone()];
        let frame = ControllerPortFrame {
            controller: id.clone(),
            port: port.id,
            slots,
        };
        let output = {
            let mut service = lock_unpoisoned(&self.live_output);
            if service.snapshot().state == crate::dto::LiveOutputState::Stopping {
                return Err("Wait for output to finish stopping before starting a test.".into());
            }
            service.test(id.clone(), controller, frame)
        };
        Ok(self.update_snapshot(|snapshot| snapshot.live_output = output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{
        GuiDocument, GuiEditCommand, LiveOutputState, SetupControllerConfig, SetupControllerPort,
        SetupGuiEdit,
    };
    use crate::project::{new_project_files, write_new_project_files};
    use camino::Utf8PathBuf;
    use std::net::UdpSocket;
    use std::time::{Duration, Instant};

    #[test]
    fn channel_test_sends_only_selected_port_and_blackouts_on_stop_suspend_and_timeout() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
        write_new_project_files(&root, &new_project_files("Output test").unwrap()).unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        let setup = state.project_session().unwrap().project.root.setup.clone();
        state.open_file_path(setup.0.document().as_str());
        let setup_request = || GuiDocumentRequest {
            project_revision: state.snapshot().project_revision,
            path: setup.0.document().to_string(),
            view: DocumentViewId::Setup,
            object_key: Some(setup.0.object().into()),
        };
        let added = state.apply_gui_edit(
            setup_request(),
            GuiEditCommand::Setup {
                edit: SetupGuiEdit::AddController {
                    config: SetupControllerConfig::ArtNet {
                        bind_address: "127.0.0.1:0".into(),
                        destination: receiver.local_addr().unwrap().to_string(),
                        broadcast: false,
                    },
                    ports: vec![
                        SetupControllerPort {
                            id: 1,
                            address: 4,
                            slot_count: 6,
                        },
                        SetupControllerPort {
                            id: 2,
                            address: 9,
                            slot_count: 6,
                        },
                    ],
                },
            },
        );
        assert!(
            matches!(added.document, GuiDocument::Setup { .. }),
            "{:?}",
            added.document
        );
        let controller = state
            .project_session()
            .unwrap()
            .project
            .setups
            .get(&setup)
            .unwrap()
            .controllers[0]
            .clone();
        let request = || GuiDocumentRequest {
            project_revision: state.snapshot().project_revision,
            path: controller.0.document().to_string(),
            view: DocumentViewId::Controller,
            object_key: Some(controller.0.object().into()),
        };
        let original = state.project_session().unwrap();
        let test = ControllerOutputTest {
            port: 2,
            start_slot: 1,
            slot_count: 3,
            value: 47,
        };
        let receive = || {
            let mut bytes = [0; 1024];
            let size = receiver.recv(&mut bytes).unwrap();
            assert_eq!(&bytes[..8], b"Art-Net\0");
            assert_eq!(u16::from_le_bytes([bytes[14], bytes[15]]), 9);
            assert_eq!(size, 24);
            bytes[18..24].to_vec()
        };
        let mut stale = request();
        stale.project_revision += 1;
        assert!(state.start_output_test(&stale, test.clone()).is_err());
        assert!(
            state
                .start_output_test(
                    &request(),
                    ControllerOutputTest {
                        slot_count: 6,
                        ..test.clone()
                    }
                )
                .is_err()
        );
        for suspend in [false, true] {
            let started = state.start_output_test(&request(), test.clone()).unwrap();
            assert_eq!(started.live_output.state, LiveOutputState::Testing);
            assert_eq!(started.live_output.active_universe_count, 1);
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                assert!(
                    Instant::now() < deadline,
                    "test did not send selected values"
                );
                let slots = receive();
                if slots == [0; 6] {
                    continue;
                }
                assert_eq!(slots, [0, 47, 47, 47, 0, 0]);
                break;
            }
            lock_unpoisoned(&state.sequence_render).unload();
            assert!(
                state
                    .set_live_output_active(true)
                    .unwrap_err()
                    .contains("prepared sequence")
            );
            assert_eq!(state.snapshot().live_output.state, LiveOutputState::Testing);
            assert_eq!(state.live_output_snapshot().state, LiveOutputState::Testing);
            assert_eq!(receive(), [0, 47, 47, 47, 0, 0]);
            if suspend {
                state.suspend_live_output();
            } else {
                assert_eq!(
                    state
                        .set_live_output_active(false)
                        .unwrap()
                        .live_output
                        .state,
                    LiveOutputState::Stopping
                );
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if receive() == [0; 6] {
                    break;
                }
                assert!(Instant::now() < deadline);
            }
            assert!(!lock_unpoisoned(&state.live_output).take_resume_after_prepare());
            let deadline = Instant::now() + Duration::from_secs(2);
            while state.live_output_snapshot().state == LiveOutputState::Stopping {
                assert!(Instant::now() < deadline, "stop was not acknowledged");
                std::thread::yield_now();
            }
            assert_eq!(
                state.live_output_snapshot().state,
                LiveOutputState::Disabled
            );
        }
        state.start_output_test(&request(), test).unwrap();
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut lit_frames = 0;
        loop {
            assert!(Instant::now() < deadline, "test did not expire");
            let slots = receive();
            if slots == [0; 6] && lit_frames == 0 {
                continue;
            }
            if slots == [0; 6] && lit_frames > 0 {
                break;
            }
            assert_eq!(slots, [0, 47, 47, 47, 0, 0]);
            lit_frames += 1;
            assert!(Instant::now() < deadline, "test did not expire");
        }
        assert!(lit_frames > 2);
        assert!(std::sync::Arc::ptr_eq(
            &original,
            &state.project_session().unwrap()
        ));
        state.shutdown_live_output();
    }
}
