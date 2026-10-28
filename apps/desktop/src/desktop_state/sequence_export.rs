use super::{DesktopState, lock_unpoisoned};
use donder_editor::dto::{
    GuiDocumentRequest, PreviewAppearance, SequenceExportOptions, SequenceExportPort,
};
use donder_model::{ControllerId, ControllerPortId, SequenceId};
use donder_project_io::ProjectSession;
use donder_runtime::PreparedShow;
use std::sync::Arc;

pub(super) fn outputs(
    session: &ProjectSession,
) -> Result<Vec<(ControllerId, ControllerPortId, u16)>, String> {
    let project = &session.project;
    let setup = project
        .get::<donder_model::Setup>(*project.root().setup.id())
        .ok_or("Project setup is missing.")?;
    let mut outputs = Vec::new();
    for source in &setup.controllers {
        let id = source.id();
        let controller = project
            .get::<donder_model::Controller>(*id)
            .ok_or("Setup controller is missing.")?;
        outputs.extend(
            controller
                .ports
                .iter()
                .map(|port| (*id, port.id, port.slot_count)),
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
        let session = self
            .project_session()
            .ok_or("Open a valid project to export.")?;
        let resolved = donder_editor::resolve_request(&session, request)?;
        let id = SequenceId::try_from(resolved.object).map_err(|_| "Open a sequence to export.")?;
        if !session.project.get::<donder_model::Sequence>(id).is_some() {
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
            .get::<donder_model::Sequence>(id)
            .ok_or("Sequence is missing.")?
            .frame_rate;
        let ports = outputs(&session)?
            .into_iter()
            .enumerate()
            .map(|(index, (controller, port, channels))| {
                Ok(SequenceExportPort {
                    index: u32::try_from(index).map_err(|_| "Too many outputs to export.")?,
                    label: {
                        let address = session.address_of(controller.into()).ok_or(
                            "An output's controller is missing from the source inventory.",
                        )?;
                        format!(
                            "{}#{} / port {}",
                            address.document(),
                            address.declared_name(),
                            port.0
                        )
                    },
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
    ) -> Result<(Arc<ProjectSession>, SequenceId, PreparedShow), String> {
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
            ports.push((*controller, *port));
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
        Ok(prepared.encode())
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
            .get::<donder_model::Sequence>(id)
            .ok_or("Sequence is missing.")?;
        let media = session
            .audio_asset(&sequence.audio)
            .and_then(|asset| asset.relative_path.file_name());
        donder_output::encode_fseq(prepared, step, media).map_err(|error| error.to_string())
    }

    /// Render the sequence as an MP4 of the Preview's front view with its song.
    pub(crate) fn prepare_video_export(
        &self,
        request: &GuiDocumentRequest,
        appearance: PreviewAppearance,
        frames_per_second: u32,
        output: &mut (impl std::io::Write + std::io::Seek),
        progress: impl FnMut(donder_editor::dto::VideoExportProgress),
    ) -> Result<(), String> {
        let (session, id) = self.sequence_export_session(request)?;
        let sequence = session
            .project
            .get::<donder_model::Sequence>(id)
            .ok_or("Sequence is missing.")?;
        let audio = session
            .audio_asset(&sequence.audio)
            .map(|asset| asset.absolute_path.clone());
        crate::preview::export_video(
            &session.project,
            &id,
            audio.as_deref(),
            appearance,
            frames_per_second,
            output,
            progress,
        )
    }
}
