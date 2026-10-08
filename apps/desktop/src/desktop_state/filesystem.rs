use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use super::{
    DesktopState, FsEntryKind, absolute_project_path, lock_unpoisoned, path_matches_or_is_child,
    valid_child_name, workspace_entries,
};
use donder_sequence_api::AppSnapshot;

impl DesktopState {
    pub(super) fn create_fs_entry(
        &self,
        parent: &str,
        name: &str,
        kind: FsEntryKind,
    ) -> AppSnapshot {
        let _authoring = self.settled_authoring();
        let Some(project) = self.project_session() else {
            return self.snapshot();
        };
        if !valid_child_name(name) {
            return self.snapshot_with_error(
                "file.create",
                parent,
                "Name must be a single path segment",
            );
        }
        let parent_path = Utf8PathBuf::from(parent);
        let relative_path = if parent.is_empty() {
            Utf8PathBuf::from(name)
        } else {
            parent_path.join(name)
        };
        let Some(path) = absolute_project_path(&project, &relative_path) else {
            return self.snapshot_with_error(
                "file.create",
                relative_path.as_str(),
                "Path is outside the loaded project",
            );
        };
        let result = {
            let _filesystem = lock_unpoisoned(&self.filesystem);
            match kind {
                FsEntryKind::File => {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent).and_then(|()| {
                            fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(&path)
                                .map(|_| ())
                        })
                    } else {
                        fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&path)
                            .map(|_| ())
                    }
                }
                FsEntryKind::Directory => fs::create_dir_all(&path),
            }
        };
        match result {
            Ok(()) => self.after_workspace_changed(None, Some(relative_path.as_str())),
            Err(error) => {
                self.snapshot_with_error("file.create", relative_path.as_str(), &error.to_string())
            }
        }
    }

    pub(super) fn after_workspace_changed(
        &self,
        old_path: Option<&str>,
        new_active_path: Option<&str>,
    ) -> AppSnapshot {
        let project = self.project_session();
        let entries = project.as_ref().map(|project| workspace_entries(project));
        if let Err(error) = self.reconcile_external_files_locked() {
            return self.snapshot_with_error("project.refresh", "", &error);
        }
        self.update_snapshot(|snapshot| {
            if let Some(entries) = entries {
                snapshot.project_entries = entries;
            }
            if let Some(old_path) = old_path {
                snapshot
                    .tabs
                    .retain(|tab| !path_matches_or_is_child(&tab.path, old_path));
                if snapshot
                    .active_file
                    .as_deref()
                    .is_some_and(|active| path_matches_or_is_child(active, old_path))
                {
                    snapshot.active_file = None;
                    snapshot.active_buffer = None;
                    snapshot.active_document_descriptor = None;
                }
            }
            snapshot.status = "Project files updated".to_string();
        });
        if let Some(path) = new_active_path
            && self
                .project_root_path()
                .and_then(|root| super::absolute_root_path(&root, Utf8Path::new(path)))
                .is_some_and(|path| path.is_file())
        {
            return self.open_file_path(path);
        }
        self.snapshot()
    }

    pub(super) fn snapshot_with_error(
        &self,
        _code: &str,
        _path: &str,
        message: &str,
    ) -> AppSnapshot {
        self.update_snapshot(|snapshot| {
            snapshot.status = message.to_string();
        })
    }
}
