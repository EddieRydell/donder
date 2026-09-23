use super::{DesktopState, lock_unpoisoned};
use crate::dto::{DocumentViewId, GuiDocumentRequest, SequenceExportPort};
use donder_language::{
    controller::{ControllerId, ControllerPortId},
    sequence::SequenceId,
};
use donder_project_io::ProjectSession;
use std::sync::Arc;

fn outputs(session: &ProjectSession) -> Result<Vec<(ControllerId, ControllerPortId, u16)>, String> {
    let project = &session.project;
    let setup = project
        .setups
        .get(&project.root.setup)
        .ok_or("Project setup is missing.")?;
    let mut outputs = Vec::new();
    for id in &setup.controllers {
        let controller = project
            .controllers
            .get(id)
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
        if request.view != DocumentViewId::Sequence {
            return Err("Open a sequence to export.".into());
        }
        let session = self
            .project_session()
            .ok_or("Open a valid project to export.")?;
        let resolved = crate::gui::resolve_request(&session, request)?;
        let id = SequenceId(resolved.identity);
        if !session.project.sequences.contains_key(&id) {
            return Err("Sequence is missing.".into());
        }
        Ok((session, id))
    }

    pub(crate) fn sequence_export_ports(
        &self,
        request: &GuiDocumentRequest,
    ) -> Result<Vec<SequenceExportPort>, String> {
        let (session, _) = self.sequence_export_session(request)?;
        outputs(&session)?
            .into_iter()
            .enumerate()
            .map(|(index, (controller, port, channels))| {
                Ok(SequenceExportPort {
                    index: u32::try_from(index).map_err(|_| "Too many outputs to export.")?,
                    label: format!(
                        "{}#{} / port {}",
                        controller.0.document(),
                        controller.0.object(),
                        port.0
                    ),
                    channels,
                })
            })
            .collect()
    }

    pub(crate) fn prepare_sequence_export(
        &self,
        request: &GuiDocumentRequest,
        selected: &[u32],
    ) -> Result<Vec<u8>, String> {
        let (session, id) = self.sequence_export_session(request)?;
        if selected.is_empty() {
            return Err("Choose at least one output.".into());
        }
        let available = outputs(&session)?;
        let mut indices = std::collections::BTreeSet::new();
        let mut ports = Vec::new();
        for index in selected {
            if !indices.insert(*index) {
                return Err("An output was selected more than once.".into());
            }
            let (controller, port, _) = available
                .get(*index as usize)
                .ok_or("Selected output is unavailable.")?;
            ports.push((controller.clone(), *port));
        }
        let prepared = donder_elaboration::PreparedSequenceOutput::prepare_selected(
            &session.project,
            &session.project.root.setup,
            &id,
            &ports,
        )
        .map_err(|error| format!("Could not prepare sequence: {error:?}"))?;
        prepared
            .encode()
            .map_err(|error| format!("Could not encode sequence: {error:?}"))
    }
}
