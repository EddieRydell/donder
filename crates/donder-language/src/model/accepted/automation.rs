//! Resolve authored automation to declaration slots during project admission.
use crate::dsl::ParamDecl;
use crate::execution::PreparedAutomation;
use crate::sequence::{AutomationTarget, Sequence};
use crate::validation::ProjectValidationError;
use crate::values::{SampleDuration, SampleTime};
use std::sync::Arc;

pub(super) fn admit(
    sequence: &Sequence,
    params: &[ParamDecl],
    target: impl Fn(&AutomationTarget) -> bool,
) -> Result<Box<[PreparedAutomation]>, ProjectValidationError> {
    sequence
        .automation_clips
        .iter()
        .flat_map(|clip| {
            clip.bindings
                .iter()
                .filter(|binding| target(&binding.target))
                .map(|binding| {
                    let name = match &binding.target {
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
                    let mut curve = clip.curve.clone();
                    curve
                        .points
                        .sort_by(|left, right| left.position.total_cmp(&right.position));
                    Ok(PreparedAutomation {
                        start: SampleTime::from_ticks(clip.start.as_micros_rounded() as u32),
                        duration: SampleDuration::from_ticks(
                            clip.duration.as_micros_rounded() as u32
                        ),
                        curve: Arc::new(curve),
                        mapping: binding.mapping.clone(),
                        param_index,
                    })
                })
        })
        .collect()
}
