//! Resolve authored automation to declaration slots during project admission.
use crate::sequence::{AutomationTarget, Sequence};
use crate::validation::{AdmissionProblem, ValidationProblem};
use donder_language::compiler::ParamDecl;
use donder_runtime_types::{AutomatedQuantity, PreparedAutomation};

use std::sync::Arc;

/// Clips bound to the same target are merged into one envelope, so playback
/// sees a single binding per target.
pub(super) fn admit(
    sequence: &Sequence,
    params: &[ParamDecl],
    target: impl Fn(&AutomationTarget) -> bool,
) -> Result<Box<[PreparedAutomation]>, ValidationProblem> {
    let mut targets = Vec::new();
    for binding in sequence
        .automation_clips
        .iter()
        .flat_map(|clip| &clip.bindings)
    {
        if target(&binding.target) && !targets.contains(&&binding.target) {
            targets.push(&binding.target);
        }
    }
    targets
        .into_iter()
        .map(|target| {
            let name = target.param();
            fn proven<T>() -> T {
                unreachable!("validation proves an automated parameter is automatable")
            }
            let index = params
                .iter()
                .position(|param| &param.name == name)
                .unwrap_or_else(proven);
            let mapping = params[index].automation_mapping().unwrap_or_else(proven);
            let param_index = u16::try_from(index)
                .map_err(|_| ValidationProblem::Admission(AdmissionProblem::TooManyParams))?;
            let envelope = sequence
                .automation_envelope(target)
                .unwrap_or_else(|| unreachable!("the target came from a binding"));
            let mut curve = envelope.curve;
            curve
                .points
                .sort_by(|left, right| left.position.total_cmp(&right.position));
            Ok(PreparedAutomation {
                start: crate::clock::sample_time(&envelope.start),
                duration: crate::clock::sample_duration(&envelope.duration),
                curve: Arc::new(curve),
                mapping,
                quantity: AutomatedQuantity::Value,
                param_index,
            })
        })
        .collect()
}
