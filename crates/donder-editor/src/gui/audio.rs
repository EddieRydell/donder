//! Where reading the assets a sequence's effects use stands.
use donder_model::{Derived, Sequence};
use donder_project_io::ProjectSession;

use crate::dto::DerivedDataStatus;

pub(super) fn derived_status(session: &ProjectSession, sequence: &Sequence) -> DerivedDataStatus {
    let derivations = session.project.sequence_derivations(sequence);
    if derivations.is_empty() {
        return DerivedDataStatus::Unused;
    }
    let mut status = DerivedDataStatus::Ready;
    for derivation in &derivations {
        match session.project.derived(derivation) {
            Some(Derived::Failed(reason)) => {
                let file = session
                    .asset(derivation.asset())
                    .map(|asset| asset.relative_path.to_string())
                    .unwrap_or_default();
                return DerivedDataStatus::Failed {
                    file,
                    reason: reason.clone(),
                };
            }
            None => status = DerivedDataStatus::Reading,
            Some(Derived::Track(_) | Derived::Font(_) | Derived::Image(_)) => {}
        }
    }
    status
}
