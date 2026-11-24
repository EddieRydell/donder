use super::DesktopState;
use donder_editor::dto::{AppSnapshot, GuiDocumentRequest, GuiFixtureCapture, GuiObjectRef};
use donder_model::ControllerId;
use donder_output::{OutputTest, TestPattern, TestSelection};

impl DesktopState {
    /// Play the capture pattern on `pixels` RGB pixels of a streamed
    /// controller from `start_channel`, in place of live output until output
    /// stops.
    pub(crate) fn start_fixture_capture(
        &self,
        request: &GuiDocumentRequest,
        controller: &GuiObjectRef,
        start_channel: u32,
        pixels: u32,
    ) -> Result<AppSnapshot, String> {
        self.stream_output_test(request, |session| {
            let controller = donder_editor::object_id_from_gui::<ControllerId>(session, controller)
                .map_err(|_| "Choose a controller to light the fixture.")?;
            let channel_count = pixels
                .checked_mul(3)
                .ok_or("Choose fewer pixels than the controller has.")?;
            OutputTest::new(
                &session.project,
                TestSelection::Channels {
                    controller,
                    start_channel,
                    channel_count,
                },
                TestPattern::Code,
            )
            .map_err(|error| error.to_string())
        })
    }

    /// Measure the open fixture's shapes from the video at `path`, which
    /// films `pixels` pixels playing the capture pattern. This reads the
    /// video only; the caller applies the shapes through a GUI edit.
    pub(crate) fn capture_fixture(
        &self,
        request: &GuiDocumentRequest,
        path: &str,
        pixels: u32,
    ) -> Result<GuiFixtureCapture, String> {
        let session = self
            .project_session()
            .ok_or("Open a valid project to capture a fixture.")?;
        let resolved = donder_editor::resolve_request(&session, request)?;
        let (definition, _) = donder_editor::open_fixture_definition(&session, &resolved)?;
        let video =
            fs_err::read(path).map_err(|error| format!("Couldn't read the video: {error}."))?;
        let capture = donder_fixture_capture::capture_fixture(&video, pixels, definition)
            .map_err(|error| error.to_string())?;
        Ok(GuiFixtureCapture {
            elements: capture
                .elements
                .iter()
                .map(donder_editor::gui_element)
                .collect(),
            found: capture.found,
            pixels,
        })
    }
}
