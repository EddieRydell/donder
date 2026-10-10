use super::{DesktopState, lock_unpoisoned};
use donder_model::{ControllerId, ControllerPortId, SequenceId};
use donder_project_io::ProjectSession;
use donder_runtime::PreparedSequence;
use donder_sequence_api::{
    DocumentViewId, GuiDocumentRequest, PreviewAppearance, SequenceExportOptions,
    SequenceExportPort,
};
use std::sync::Arc;

pub(super) fn outputs(
    session: &ProjectSession,
) -> Result<Vec<(ControllerId, ControllerPortId, u16)>, String> {
    let project = &session.project;
    let setup = project
        .setup(project.root().setup.id())
        .ok_or("Project setup is missing.")?;
    let mut outputs = Vec::new();
    for source in &setup.controllers {
        let id = source.id();
        let controller = project
            .controller(id)
            .ok_or("Setup controller is missing.")?;
        outputs.extend(
            controller
                .ports
                .iter()
                .map(|port| (id.clone(), port.id, port.slot_count)),
        );
    }
    Ok(outputs)
}

impl DesktopState {
    pub(super) fn sequence_export_session(
        &self,
        request: &GuiDocumentRequest,
    ) -> Result<(Arc<ProjectSession>, SequenceId), String> {
        let _authoring = lock_unpoisoned(&self.authoring);
        if request.project_revision != self.snapshot().project_revision {
            return Err(
                "The project changed. Reopen sequence export to use the current outputs.".into(),
            );
        }
        if request.view != DocumentViewId::Sequence {
            return Err("Open a sequence to export.".into());
        }
        let session = self
            .project_session()
            .ok_or("Open a valid project to export.")?;
        let resolved = donder_editor::resolve_request(&session, request)?;
        let id = SequenceId(resolved.object_identity());
        if !session.project.sequence(&id).is_some() {
            return Err("Sequence is missing.".into());
        }
        Ok((session, id))
    }

    pub(crate) fn sequence_export_options(
        &self,
        request: &GuiDocumentRequest,
    ) -> Result<SequenceExportOptions, String> {
        let (session, id) = self.sequence_export_session(request)?;
        let frame_rate = session
            .project
            .sequence(&id)
            .ok_or("Sequence is missing.")?
            .frame_rate;
        let ports = outputs(&session)?
            .into_iter()
            .enumerate()
            .map(|(index, (controller, port, channels))| {
                Ok(SequenceExportPort {
                    index: u32::try_from(index).map_err(|_| "Too many outputs to export.")?,
                    label: format!(
                        "{}#{} / port {}",
                        controller.0.document(),
                        controller.0.root_source().object(),
                        port.0
                    ),
                    channels,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(SequenceExportOptions {
            ports,
            fseq_step_millis: donder_output::FseqStep::nearest(frame_rate).millis(),
        })
    }

    /// Prepare the sequence for the selected outputs, in selection order.
    fn prepare_selected_outputs(
        &self,
        request: &GuiDocumentRequest,
        selected: &[u32],
    ) -> Result<(Arc<ProjectSession>, SequenceId, PreparedSequence), String> {
        let (session, id) = self.sequence_export_session(request)?;
        if selected.is_empty() {
            return Err("Choose at least one output.".into());
        }
        let available = outputs(&session)?;
        let mut indices = std::collections::BTreeSet::new();
        let mut ports = Vec::new();
        for index in selected {
            if !indices.insert(*index) {
                continue;
            }
            let (controller, port, _) = available
                .get(*index as usize)
                .ok_or("Selected output is unavailable.")?;
            ports.push((controller.clone(), *port));
        }
        let prepared = donder_elaboration::prepare(
            &session.project,
            &id,
            donder_elaboration::PrepareOutputs::Ports(&ports),
        )
        .ok_or("The sequence or selected output is unavailable.")?;
        Ok((session, id, prepared))
    }

    pub(crate) fn prepare_sequence_export(
        &self,
        request: &GuiDocumentRequest,
        selected: &[u32],
    ) -> Result<Vec<u8>, String> {
        let (_, _, prepared) = self.prepare_selected_outputs(request, selected)?;
        donder_runtime::encode_sequence(&prepared)
            .map_err(|error| format!("Could not encode sequence: {error:?}"))
    }

    pub(crate) fn prepare_fseq_export(
        &self,
        request: &GuiDocumentRequest,
        selected: &[u32],
        step_millis: u8,
    ) -> Result<Vec<u8>, String> {
        let step = donder_output::FseqStep::from_millis(step_millis)
            .ok_or("The FSEQ frame step must be at least 1 ms.")?;
        let (session, id, prepared) = self.prepare_selected_outputs(request, selected)?;
        let sequence = session
            .project
            .sequence(&id)
            .ok_or("Sequence is missing.")?;
        let media = session
            .audio_asset(id.0.document_id(), &sequence.audio)
            .and_then(|asset| asset.relative_path.file_name());
        donder_output::encode_fseq(prepared, step, media).map_err(|error| error.to_string())
    }

    /// Render the sequence as an MP4 of the Preview's front view with its song.
    pub(crate) fn prepare_video_export(
        &self,
        request: &GuiDocumentRequest,
        appearance: PreviewAppearance,
        frames_per_second: u32,
        output: impl std::io::Write,
        progress: impl FnMut(donder_preview::VideoProgress),
    ) -> Result<(), String> {
        let (session, id) = self.sequence_export_session(request)?;
        let sequence = session
            .project
            .sequence(&id)
            .ok_or("Sequence is missing.")?;
        let audio = session
            .audio_asset(id.0.document_id(), &sequence.audio)
            .map(|asset| asset.absolute_path.clone());
        let options = donder_preview::VideoOptions {
            width: VIDEO_WIDTH,
            height: VIDEO_HEIGHT,
            frames_per_second,
            background_rgb: appearance.background_rgb,
            unlit_rgb: appearance.unlit_rgb,
            style: donder_preview::ViewStyle {
                canvas_fill_ratio: appearance.canvas_fill_ratio,
                minimum_radius_pixels: appearance.minimum_radius_pixels,
            },
        };
        donder_preview::export_video(
            &session.project,
            &id,
            audio.as_deref(),
            &options,
            output,
            progress,
        )
        .map_err(|error| error.to_string())
    }
}

/// Exported videos are 1080p, the size phones and sharing sites expect.
const VIDEO_WIDTH: u32 = 1920;
const VIDEO_HEIGHT: u32 = 1080;
