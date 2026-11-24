use super::{DesktopState, lock_unpoisoned};
use donder_editor::dto::{AppSnapshot, GuiDocumentRequest, OutputTestPattern, OutputTestTarget};
use donder_model::{ControllerId, FixtureInstanceId, LayoutId, PatchId, PixelRouteId};
use donder_output::{OutputTest, TestPattern, TestSelection};
use donder_project_io::ProjectSession;

impl DesktopState {
    /// Stream a test pattern on what the open document's `target` denotes,
    /// in place of live output, until output stops.
    pub(crate) fn start_output_test(
        &self,
        request: &GuiDocumentRequest,
        target: OutputTestTarget,
        pattern: OutputTestPattern,
        reversed: bool,
    ) -> Result<AppSnapshot, String> {
        self.stream_output_test(request, |session| {
            let object = donder_editor::resolve_request(session, request)?.object;
            let selection = match target {
                OutputTestTarget::Channels {
                    start_channel,
                    channel_count,
                } => TestSelection::Channels {
                    controller: ControllerId::try_from(object)
                        .map_err(|_| "Open a controller to test its channels.")?,
                    start_channel,
                    channel_count,
                },
                OutputTestTarget::LayoutItem { item } => TestSelection::LayoutItem {
                    layout: LayoutId::try_from(object)
                        .map_err(|_| "Open a layout to test its fixtures.")?,
                    item: FixtureInstanceId(item),
                },
                OutputTestTarget::Route { route } => TestSelection::Route {
                    patch: PatchId::try_from(object)
                        .map_err(|_| "Open a patch to test its outputs.")?,
                    route: PixelRouteId(route),
                },
            };
            let pattern = match pattern {
                OutputTestPattern::Solid { red, green, blue } => {
                    TestPattern::Solid([red, green, blue])
                }
                OutputTestPattern::Count => TestPattern::Count,
                OutputTestPattern::Order => TestPattern::Order,
                OutputTestPattern::Chase => TestPattern::Chase,
                OutputTestPattern::Rise => TestPattern::Rise,
            };
            let mut test = OutputTest::new(session.project(), selection, pattern)
                .map_err(|error| error.to_string())?;
            if reversed {
                test.reverse();
            }
            Ok(test)
        })
    }

    /// Stream the test `build` makes from the current project in place of
    /// live output, until output stops.
    pub(super) fn stream_output_test(
        &self,
        request: &GuiDocumentRequest,
        build: impl FnOnce(&ProjectSession) -> Result<OutputTest, String>,
    ) -> Result<AppSnapshot, String> {
        let _authoring = lock_unpoisoned(&self.authoring);
        if request.project_revision != self.snapshot().project_revision {
            return Err("The project changed. Start the test again.".into());
        }
        let session = self
            .project_session()
            .ok_or("Open a valid project to test outputs.")?;
        let test = build(&session)?;
        let output = {
            let mut service = lock_unpoisoned(&self.live_output);
            if service.stopping() {
                return Err("Wait for output to finish stopping before starting a test.".into());
            }
            service.test(test)
        };
        Ok(self.update_snapshot(|snapshot| snapshot.live_output = output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{new_test_project_files, write_new_project_files};
    use camino::Utf8PathBuf;
    use donder_editor::dto::{
        GuiDocument, GuiEditCommand, GuiUniverses, LiveOutputState, SetupControllerConfig,
        SetupGuiEdit,
    };
    use std::net::UdpSocket;
    use std::time::{Duration, Instant};

    #[test]
    fn channel_test_sends_only_selected_port_and_blackouts_on_stop_and_suspend() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temporary.path().join("show")).unwrap();
        write_new_project_files(&root, &new_test_project_files("Output test").unwrap()).unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        let setup = *state.project_session().unwrap().project().root().setup.id();
        let setup_address = state.project_session().unwrap().address_of(setup);
        state.open_file_path(setup_address.document().as_str());
        let setup_request = || GuiDocumentRequest {
            owned_path: setup_address.owned_path().iter().map(Into::into).collect(),
            project_revision: state.snapshot().project_revision,
            path: setup_address.document().to_string(),
            object_key: setup_address.root_source().object().into(),
        };
        let added = state.apply_gui_edit(
            setup_request(),
            GuiEditCommand::Setup {
                edit: SetupGuiEdit::AddController {
                    name: "output_test".into(),
                    config: SetupControllerConfig::ArtNet {
                        bind_address: "127.0.0.1:0".into(),
                        destination: receiver.local_addr().unwrap().to_string(),
                        broadcast: false,
                        universes: GuiUniverses {
                            start: 8,
                            count: 2,
                            channels: 6,
                        },
                        fseq_channel: None,
                    },
                    ports: Vec::new(),
                },
            },
        );
        assert!(
            matches!(added.document, GuiDocument::Setup { .. }),
            "{:?}",
            added.document
        );
        let controller = *state
            .project_session()
            .unwrap()
            .project()
            .get::<donder_model::Setup>(setup)
            .unwrap()
            .controllers[0]
            .id();
        let controller_address = state.project_session().unwrap().address_of(controller);
        let request = || GuiDocumentRequest {
            owned_path: controller_address
                .owned_path()
                .iter()
                .map(Into::into)
                .collect(),
            project_revision: state.snapshot().project_revision,
            path: controller_address.document().to_string(),
            object_key: controller_address.root_source().object().into(),
        };
        let original = state.project_session().unwrap();
        // Universe 9's second to fourth channels.
        let test = OutputTestTarget::Channels {
            start_channel: 7,
            channel_count: 3,
        };
        let pattern = OutputTestPattern::Solid {
            red: 47,
            green: 47,
            blue: 47,
        };
        // Universe 9's channels; universe 8 is all zero.
        let receive = || {
            loop {
                let mut bytes = [0; 1024];
                let size = receiver.recv(&mut bytes).unwrap();
                assert_eq!(&bytes[..8], b"Art-Net\0");
                assert_eq!(size, 24);
                match u16::from_le_bytes([bytes[14], bytes[15]]) {
                    8 => assert_eq!(bytes[18..24], [0; 6]),
                    universe => {
                        assert_eq!(universe, 9);
                        return bytes[18..24].to_vec();
                    }
                }
            }
        };
        let mut stale = request();
        stale.project_revision += 1;
        assert!(
            state
                .start_output_test(&stale, test, pattern, false)
                .is_err()
        );
        assert!(
            state
                .start_output_test(
                    &request(),
                    OutputTestTarget::Channels {
                        start_channel: 7,
                        channel_count: 6,
                    },
                    pattern,
                    false,
                )
                .is_err()
        );
        for suspend in [false, true] {
            let started = state
                .start_output_test(&request(), test, pattern, false)
                .unwrap();
            assert_eq!(started.live_output.state, LiveOutputState::Testing);
            assert_eq!(started.live_output.active_universe_count, 2);
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
            state.set_live_output_active(true);
            assert_eq!(state.snapshot().live_output.state, LiveOutputState::Testing);
            assert_eq!(state.live_output_snapshot().state, LiveOutputState::Testing);
            assert_eq!(receive(), [0, 47, 47, 47, 0, 0]);
            if suspend {
                state.stop_live_output();
            } else {
                assert_eq!(
                    state.set_live_output_active(false).live_output.state,
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
            assert!(!lock_unpoisoned(&state.live_output).take_pending_start());
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
        assert!(std::sync::Arc::ptr_eq(
            &original,
            &state.project_session().unwrap()
        ));
        state.shutdown_live_output();
    }
}
