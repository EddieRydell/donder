//! Resolve authored automation to declaration slots during project admission.
use crate::dsl::ParamDecl;
use crate::execution::PreparedAutomation;
use crate::sequence::{AutomationTarget, Sequence};
use crate::validation::ProjectValidationError;
use crate::values::{SampleDuration, SampleTime};
use std::sync::Arc;

/// Clips bound to the same target are merged into one envelope, so playback
/// sees a single binding per target.
pub(super) fn admit(
    sequence: &Sequence,
    params: &[ParamDecl],
    target: impl Fn(&AutomationTarget) -> bool,
) -> Result<Box<[PreparedAutomation]>, ProjectValidationError> {
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
            let name = match target {
                AutomationTarget::EffectParam { param, .. }
                | AutomationTarget::CompositionNodeParam { param, .. } => param,
            };
            let param_index = params
                .iter()
                .position(|param| &param.name == name)
                .and_then(|index| u16::try_from(index).ok())
                .ok_or_else(|| {
                    ProjectValidationError::InvalidRelationship(
                        "Unknown automation parameter".into(),
                    )
                })?;
            let envelope = sequence.automation_envelope(target).ok_or_else(|| {
                ProjectValidationError::InvalidRelationship("Automation target has no clip".into())
            })?;
            let mut curve = envelope.curve;
            curve
                .points
                .sort_by(|left, right| left.position.total_cmp(&right.position));
            Ok(PreparedAutomation {
                start: SampleTime::from_ticks(envelope.start.as_micros_rounded() as u32),
                duration: SampleDuration::from_ticks(envelope.duration.as_micros_rounded() as u32),
                curve: Arc::new(curve),
                mapping: envelope.mapping.clone(),
                param_index,
            })
        })
        .collect()
}
