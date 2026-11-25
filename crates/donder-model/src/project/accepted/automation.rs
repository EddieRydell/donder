//! Resolve authored automation to declaration slots during project admission.
use crate::composition::Composition;
use crate::sequence::AutomationTarget;
use crate::validation::{AdmissionProblem, ValidationProblem};
use donder_language::compiler::ParamDecl;
use donder_runtime_types::{AutomationSource, PreparedAutomation};

use std::sync::Arc;

/// Envelopes bound to the same target are merged into one curve, so playback
/// sees a single binding per target.
pub(super) fn admit(
    composition: &Composition,
    params: &[ParamDecl],
    target: impl Fn(&AutomationTarget) -> bool,
) -> Result<Box<[PreparedAutomation]>, ValidationProblem> {
    let mut targets = Vec::new();
    for binding in composition
        .clips
        .iter()
        .filter_map(|clip| clip.envelope())
        .flat_map(|envelope| &envelope.bindings)
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
            let envelope = composition
                .automation_envelope(target)
                .unwrap_or_else(|| unreachable!("the target came from a binding"));
            let mut curve = envelope.curve;
            curve
                .points
                .sort_by(|left, right| left.position.total_cmp(&right.position));
            Ok(PreparedAutomation {
                source: AutomationSource::Envelope {
                    start: crate::clock::sample_time(&envelope.start),
                    duration: crate::clock::sample_duration(&envelope.duration),
                    curve: Arc::new(curve),
                },
                mapping,
                param_index,
            })
        })
        .collect()
}
