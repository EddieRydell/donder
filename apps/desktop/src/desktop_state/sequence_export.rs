use super::{DesktopState, lock_unpoisoned};
use donder_editor::dto::{
    ExportProgress, GuiDocumentRequest, PreviewAppearance, SequenceExportOptions,
    SequenceExportPort,
};
use donder_model::{ControllerId, ControllerOutput, SequenceId};
use donder_project_io::ProjectSession;
use donder_runtime::PreparedShow;
use std::num::NonZeroU32;
use std::sync::Arc;

/// An output of the setup's controllers: an E1.31 or Art-Net controller's
/// universes, or one Donder port.
struct ExportOutput {
    controller: ControllerId,
    output: ControllerOutput,
    label: String,
    width: u32,
    fseq_channel: Option<NonZeroU32>,
}

fn outputs(session: &ProjectSession) -> Result<Vec<ExportOutput>, String> {
    let project = session.project();
    let mut outputs = Vec::new();
    for controller in project.active_setup().controllers() {
        let id = controller.id();
        let address = session.address_of(id);
        for (output, width) in controller.outputs() {
            let label = controller.output_label(output);
            outputs.push(ExportOutput {
                controller: id,
                output,
                label: format!(
                    "{}#{} / {label}",
                    address.document(),
                    address.declared_name()
                ),
                width,
                fseq_channel: controller.fseq_channel(),
            });
        }
    }
    Ok(outputs)
}

impl DesktopState {
    fn sequence_export_session(
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
        Ok((session, id))
    }

    /// The sequence's declared name, which export files are named after.
    pub(crate) fn sequence_export_name(
        &self,
        request: &GuiDocumentRequest,
    ) -> Result<String, String> {
        let (session, id) = self.sequence_export_session(request)?;
        Ok(session.address_of(id).declared_name().to_string())
    }

    pub(crate) fn sequence_export_options(
        &self,
        request: &GuiDocumentRequest,
    ) -> Result<SequenceExportOptions, String> {
        let (session, id) = self.sequence_export_session(request)?;
        let Some(sequence) = session.project().get::<donder_model::Sequence>(id) else {
            unreachable!("the export request resolved to this sequence");
        };
        let frame_rate = sequence.frame_rate;
        let ports = outputs(&session)?
            .into_iter()
            .enumerate()
            .map(|(index, output)| {
                Ok(SequenceExportPort {
                    index: u32::try_from(index).map_err(|_| "Too many outputs to export.")?,
                    label: output.label,
                    channels: output.width,
                    fseq_channel: output.fseq_channel.map(NonZeroU32::get),
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(SequenceExportOptions {
            ports,
            fseq_step_millis: donder_output::FseqStep::nearest(frame_rate).millis(),
            fseq_smooth_step_millis: donder_output::FseqStep::SMOOTH_MILLIS,
        })
    }

    /// Prepare the sequence for the selected outputs, in selection order.
    fn prepare_selected_outputs(
        &self,
        request: &GuiDocumentRequest,
        selected: &[u32],
    ) -> Result<(Arc<ProjectSession>, SequenceId, PreparedShow, Vec<String>), String> {
        let (session, id) = self.sequence_export_session(request)?;
        if selected.is_empty() {
            return Err("Choose at least one output.".into());
        }
        let available = outputs(&session)?;
        let mut indices = std::collections::BTreeSet::new();
        let mut ports = Vec::new();
        let mut labels = Vec::new();
        for index in selected {
            if !indices.insert(*index) {
                continue;
            }
            let output = available
                .get(*index as usize)
                .ok_or("Selected output is unavailable.")?;
            ports.push((output.controller, output.output));
            labels.push(output.label.clone());
        }
        let prepared = donder_elaboration::prepare(
            session.project(),
            &id,
            donder_elaboration::Retain::Outputs(&ports),
        )
        .ok_or("The sequence or selected output is unavailable.")?;
        Ok((session, id, prepared, labels))
    }

    pub(crate) fn prepare_show_export(
        &self,
        request: &GuiDocumentRequest,
        selected: &[u32],
    ) -> Result<Vec<u8>, String> {
        let (_, _, prepared, _) = self.prepare_selected_outputs(request, selected)?;
        Ok(prepared.encode())
    }

    pub(crate) fn prepare_fseq_export(
        &self,
        request: &GuiDocumentRequest,
        selected: &[u32],
        step_millis: u8,
        mut progress: impl FnMut(ExportProgress),
    ) -> Result<Vec<u8>, String> {
        let step = donder_output::FseqStep::from_millis(step_millis)
            .ok_or("The FSEQ frame step must be at least 1 ms.")?;
        progress(ExportProgress::Preparing);
        let (session, id, prepared, labels) = self.prepare_selected_outputs(request, selected)?;
        let Some(sequence) = session.project().get::<donder_model::Sequence>(id) else {
            unreachable!("the export request resolved to this sequence");
        };
        let media = session
            .audio_asset(&sequence.audio)
            .and_then(|asset| asset.relative_path.file_name());
        donder_output::encode_fseq(
            prepared,
            donder_output::setup_fseq_channel(session.project()),
            step,
            media,
            |completed, total| progress(ExportProgress::Rendering { completed, total }),
        )
        .map_err(|error| error.describe(|index| &labels[index]).to_string())
    }

    /// Render the sequence as an MP4 of the Preview's front view with its song.
    pub(crate) fn prepare_video_export(
        &self,
        request: &GuiDocumentRequest,
        appearance: PreviewAppearance,
        frames_per_second: u32,
        output: &mut (impl std::io::Write + std::io::Seek),
        progress: impl FnMut(ExportProgress),
    ) -> Result<(), String> {
        let (session, id) = self.sequence_export_session(request)?;
        let Some(sequence) = session.project().get::<donder_model::Sequence>(id) else {
            unreachable!("the export request resolved to this sequence");
        };
        let audio = session
            .audio_asset(&sequence.audio)
            .map(|asset| asset.absolute_path.clone());
        crate::preview::export_video(
            session.project(),
            &id,
            audio.as_deref(),
            appearance,
            frames_per_second,
            output,
            progress,
        )
    }
}
