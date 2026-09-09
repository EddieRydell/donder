use super::workspace_state::WorkingDocument;
use super::{
    DesktopState, LoadedProject, descriptor_for_path, lock_unpoisoned, project_diagnostics,
    recovery_workspace_entries, workspace_entries,
};
use crate::dto::{
    AppSnapshot, EditorViewMode, GuiDocument, GuiDocumentRequest, ProjectHealth, SequenceAudio,
    SidebarView,
};
use camino::{Utf8Path, Utf8PathBuf};
use dawn_project_io::{ProjectCheckReport, ProjectSession};
use std::sync::Arc;

impl DesktopState {
    pub(super) fn load_working_copy(&self, root: &Utf8Path) -> AppSnapshot {
        self.working_copy.invalidate_pending();
        let root = root.to_path_buf();
        self.disable_live_output();
        self.invalidate_prepared_project();
        lock_unpoisoned(&self.gui_history).clear();
        let sources = match dawn_project_io::project_source_texts(&root) {
            Ok(sources) => sources,
            Err(error) => {
                return self.snapshot_with_error("project.open", root.as_str(), &error.to_string());
            }
        };
        // Open from a single captured source input, including the package files.
        let report = dawn_project_io::check_package_with_overrides(&root, &sources);
        let valid_paths = report
            .recovery
            .documents
            .keys()
            .map(ToString::to_string)
            .collect();
        let restore = self
            .persistence
            .restore_for_project(root.as_str(), &valid_paths);
        let mut paths = restore
            .as_ref()
            .map(|value| value.session.tabs.clone())
            .unwrap_or_default();
        if paths.is_empty() {
            if let Some(entrypoint) = report
                .recovery
                .manifest
                .as_ref()
                .and_then(|manifest| manifest.project.as_ref())
            {
                paths.push(entrypoint.entrypoint.clone());
            } else {
                paths.push(dawn_package::MANIFEST_FILE.into());
            }
        }
        let mut documents = sources
            .into_iter()
            .map(|(path, text)| {
                let document = WorkingDocument::new(&path, text);
                (path, document)
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        for path in &paths {
            if !documents.contains_key(Utf8Path::new(path))
                && let Ok(text) = std::fs::read_to_string(root.join(path))
            {
                documents.insert(
                    Utf8PathBuf::from(path),
                    WorkingDocument::new(Utf8Path::new(path), text),
                );
            }
        }
        let tabs: Vec<_> = paths
            .into_iter()
            .map(Utf8PathBuf::from)
            .filter(|path| documents.contains_key(path))
            .collect();
        let active = restore
            .as_ref()
            .and_then(|value| value.session.active_file.clone())
            .filter(|path| tabs.contains(&Utf8PathBuf::from(path)))
            .or_else(|| tabs.first().map(ToString::to_string));
        {
            let mut workspace = lock_unpoisoned(&self.workspace);
            workspace.view.project_epoch += 1;
            workspace.view.state_revision += 1;
            workspace.view.project_revision += 1;
            workspace.typed_revision = None;
            workspace.project = LoadedProject::Checking;
            workspace.view.project_health = ProjectHealth::Checking;
            workspace.view.active_document_descriptor = None;
            workspace.view.project_root = Some(root.to_string());
            workspace.documents = documents;
            workspace.render_target = None;
            workspace.tabs = tabs;
            workspace.view.active_file = active;
            workspace.view.workspace_explorer = restore
                .as_ref()
                .map(|value| value.session.workspace_explorer.clone())
                .unwrap_or_default();
        }
        self.apply_analysis(report);
        self.start_filesystem_watcher(&root);
        if self.snapshot().project_health == ProjectHealth::Invalid {
            self.focus_first_diagnostic();
        }
        if let Some(restore) = restore.filter(|restore| !restore.stale_tabs.is_empty()) {
            self.update_snapshot(|snapshot| {
                snapshot.status = format!("Skipped missing tabs: {}", restore.stale_tabs.join(", "))
            });
        }
        self.snapshot()
    }

    pub(super) fn apply_analysis(&self, report: ProjectCheckReport) {
        let diagnostics = project_diagnostics(&report);
        let root = report.recovery.root.clone();
        let package_sources = lock_unpoisoned(&self.workspace)
            .documents
            .iter()
            .filter(|(path, _)| {
                matches!(
                    path.as_str(),
                    dawn_package::MANIFEST_FILE | dawn_package::LOCK_FILE
                )
            })
            .map(|(path, document)| (path.clone(), document.buffer.text.clone()))
            .collect();
        let package = super::packages::package_status_with_sources(
            &root,
            report.session.as_ref(),
            &package_sources,
        );
        let session = report.session.map(Arc::new);
        let entries = session.as_ref().map_or_else(
            || recovery_workspace_entries(&report.recovery),
            |session| workspace_entries(session),
        );
        let conflicted = lock_unpoisoned(&self.workspace)
            .documents
            .values()
            .any(|doc| doc.buffer.external_state != crate::dto::BufferExternalState::Current);
        let session = session.filter(|_| !conflicted);
        let active = self.snapshot().active_file;
        let descriptor = session.as_ref().and_then(|session| {
            active
                .as_deref()
                .and_then(|path| descriptor_for_path(session, Utf8Path::new(path)))
        });
        {
            let mut workspace = lock_unpoisoned(&self.workspace);
            if let Some(session) = &session {
                for (path, document) in &mut workspace.documents {
                    if let Some(source) =
                        crate::source_documents::document_for_editor_path(session, path)
                    {
                        document.buffer.read_only = !session.source.is_project_owned(&source);
                    }
                }
            }
            workspace.typed_revision = session.as_ref().map(|_| workspace.view.project_revision);
            workspace.project = match &session {
                Some(session) => LoadedProject::Ready(Arc::clone(session)),
                None => LoadedProject::Invalid,
            };
        }
        if session.is_none() {
            self.invalidate_prepared_project();
        }
        self.update_snapshot(|snapshot| {
            snapshot.project_health = if session.is_some() {
                ProjectHealth::Ready
            } else {
                ProjectHealth::Invalid
            };
            snapshot.project_entries = entries;
            snapshot.active_document_descriptor = descriptor;
            snapshot.package = package;
            snapshot.diagnostics = diagnostics;
            snapshot.status = if session.is_some() {
                "Project checked".into()
            } else {
                "Fix the project errors in Text to use GUI editing".into()
            };
            if session.is_none() {
                snapshot.settings.editor_view_mode = EditorViewMode::Text;
                snapshot.workspace_layout.active_sidebar_view = SidebarView::Problems;
                snapshot.workspace_layout.sidebar_collapsed = false;
            }
        });
        if let Some(session) = session {
            self.schedule_render_refresh(session);
        }
    }

    pub(super) fn focus_first_diagnostic(&self) {
        let snapshot = self.snapshot();
        if let Some(diagnostic) = snapshot
            .diagnostics
            .iter()
            .find(|item| matches!(item.severity, crate::dto::DiagnosticSeverity::Error))
        {
            let path = Utf8Path::new(&diagnostic.path);
            let relative = snapshot
                .project_root
                .as_ref()
                .and_then(|root| path.strip_prefix(root).ok())
                .unwrap_or(path);
            self.open_file_path(relative.as_str());
        }
    }

    pub(super) fn project_session(&self) -> Option<Arc<ProjectSession>> {
        let workspace = lock_unpoisoned(&self.workspace);
        if workspace.typed_revision != Some(workspace.view.project_revision) {
            return None;
        }
        match &workspace.project {
            LoadedProject::Ready(session) => Some(Arc::clone(session)),
            _ => None,
        }
    }

    pub(super) fn project_root_path(&self) -> Option<Utf8PathBuf> {
        lock_unpoisoned(&self.workspace)
            .view
            .project_root
            .as_ref()
            .map(Utf8PathBuf::from)
    }

    pub(super) fn project_revision(&self) -> u64 {
        self.snapshot().project_revision.into()
    }
    pub(super) fn resolve_sequence_audio(
        &self,
        request: &GuiDocumentRequest,
    ) -> Option<SequenceAudio> {
        let project = self.project_session();
        match crate::gui::project_gui_document(project.as_deref(), request) {
            GuiDocument::Sequence { document } => document.audio,
            _ => None,
        }
    }

    pub(super) fn resolve_sequence_id(
        &self,
        request: &GuiDocumentRequest,
    ) -> Option<dawn_language::sequence::SequenceId> {
        if request.project_revision != self.snapshot().project_revision
            || request.view != crate::dto::DocumentViewId::Sequence
        {
            return None;
        }
        let project = self.project_session()?;
        let resolved = crate::gui::resolve_request(&project, request).ok()?;
        let id = dawn_language::sequence::SequenceId(resolved.identity);
        project.project.sequences.contains_key(&id).then_some(id)
    }
}
