//! Deriving the data effects read from asset files, audio analyses and font
//! rasters, in the background. Results are derived data: they replace the
//! current snapshot under a new revision, so renders, devices and the GUI
//! follow, but add no history and dirty no document.
use super::{DesktopState, lock_unpoisoned};
use camino::Utf8PathBuf;
use donder_model::{AssetId, Derivation, Derived};
use donder_project_io::ProjectSession;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// The derivations a project has no result for, by asset file.
pub(crate) struct DerivationPayload {
    project_epoch: u32,
    files: Vec<(Utf8PathBuf, Vec<Derivation>)>,
}

/// Results the worker keeps by file and settings, so an undo, redo or reload
/// that brings back a derivation takes its result at once.
#[derive(Default)]
pub(crate) struct DerivedResults(HashMap<(Utf8PathBuf, Derivation), Derived>);

impl DerivedResults {
    /// The result of each payload derivation, deriving the ones not kept.
    pub(crate) fn derive(
        &mut self,
        payload: DerivationPayload,
    ) -> (u32, Vec<(Derivation, Derived)>) {
        let mut results = Vec::new();
        for (path, derivations) in payload.files {
            // Asset ids differ between loads; the file and settings do not.
            let key = |derivation: &Derivation| (path.clone(), derivation.with_asset(AssetId(0)));
            let missing: Vec<Derivation> = derivations
                .iter()
                .filter(|derivation| !self.0.contains_key(&key(derivation)))
                .cloned()
                .collect();
            if !missing.is_empty() {
                let derived = donder_elaboration::derive(path.as_std_path(), &missing);
                for (derivation, derived) in missing.iter().zip(derived) {
                    self.0.insert(key(derivation), derived);
                }
            }
            for derivation in derivations {
                let derived = self.0[&key(&derivation)].clone();
                results.push((derivation, derived));
            }
        }
        (payload.project_epoch, results)
    }
}

impl DesktopState {
    /// Derive the data `session`'s effects read that it has no result for.
    pub(super) fn schedule_derivations(&self, session: &ProjectSession) {
        let mut files: Vec<(Utf8PathBuf, Vec<Derivation>)> = Vec::new();
        for derivation in session.project.pending_derivations() {
            let Some(asset) = session.asset(derivation.asset()) else {
                continue;
            };
            match files
                .iter_mut()
                .find(|(path, _)| *path == asset.absolute_path)
            {
                Some((_, derivations)) => derivations.push(derivation),
                None => files.push((asset.absolute_path.clone(), vec![derivation])),
            }
        }
        if files.is_empty() {
            return;
        }
        self.derivations.schedule(DerivationPayload {
            project_epoch: self.snapshot().project_epoch,
            files,
        });
    }

    /// Give the current project the derived data it still lacks.
    pub(crate) fn complete_derivations(
        &self,
        project_epoch: u32,
        results: Vec<(Derivation, Derived)>,
    ) {
        let _authoring = lock_unpoisoned(&self.authoring);
        if self.snapshot().project_epoch != project_epoch {
            return;
        }
        let Some(session) = self.project_session() else {
            return;
        };
        let pending = session.project.pending_derivations();
        let mut project = session.project.clone();
        let mut changed = false;
        for (derivation, derived) in results {
            if !pending.contains(&derivation) {
                continue;
            }
            match project.with_derived(derivation, derived) {
                Ok(next) => {
                    project = next;
                    changed = true;
                }
                Err(error) => {
                    self.snapshot_with_error(&error.to_string());
                    return;
                }
            }
        }
        if !changed {
            return;
        }
        let session = Arc::new(ProjectSession {
            project,
            ..(*session).clone()
        });
        if let Err(error) =
            self.accept_gui_sources(session, BTreeMap::new(), "Read the effects' assets")
        {
            self.snapshot_with_error(&error);
        }
    }
}
