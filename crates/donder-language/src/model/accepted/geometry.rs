//! The runtime geometry contract is established with the authored layout, not
//! rediscovered while preparing a selected sequence.
use super::DonderProject;
use crate::layout::{FixtureInstanceId, LayoutId};
use crate::validation::ProjectValidationError;
use donder_runtime::FixtureGeometry;
use indexmap::IndexMap;
use std::sync::Arc;

pub(super) type LayoutGeometry = Arc<[(FixtureInstanceId, FixtureGeometry)]>;

pub(super) fn admit(
    project: &DonderProject,
    previous: Option<&DonderProject>,
) -> Result<IndexMap<LayoutId, LayoutGeometry>, ProjectValidationError> {
    let previous = previous.filter(|previous| {
        let fixtures = &project.definitions().fixtures;
        let previous_fixtures = &previous.definitions().fixtures;
        std::ptr::eq(fixtures, previous_fixtures) || fixtures == previous_fixtures
    });
    // Sequence-only edits need neither definition expansion nor transformed
    // coordinates. Prepare definitions lazily, once a layout needs new geometry.
    let mut definitions = None;
    project
        .layouts()
        .map(|layout| {
            if let Some(previous) = previous
                && previous.layout(&layout.id).is_some_and(|previous_layout| {
                    std::ptr::eq(layout, previous_layout) || layout == previous_layout
                })
            {
                return Ok((
                    layout.id.clone(),
                    Arc::clone(&previous.accepted_inputs.layouts[&layout.id]),
                ));
            }
            let definitions = definitions.get_or_insert_with(|| {
                crate::geometry::PreparedFixtureDefinitions::prepare(
                    &project.definitions().fixtures,
                )
            });
            let geometry = definitions.prepare_layout(layout);
            let fixtures = geometry
                .instances
                .into_iter()
                .map(|instance| {
                    let positions = instance
                        .pixels
                        .iter()
                        .map(|pixel| {
                            let point = instance.transform.transform_point3(pixel.position);
                            [point.x, point.y]
                        })
                        .collect();
                    let geometry = FixtureGeometry::admit(positions).ok_or_else(|| {
                        ProjectValidationError::InvalidRelationship(
                            "Transformed fixture geometry is outside the playback coordinate range"
                                .into(),
                        )
                    })?;
                    Ok((instance.id, geometry))
                })
                .collect::<Result<LayoutGeometry, ProjectValidationError>>()?;
            Ok((layout.id.clone(), fixtures))
        })
        .collect()
}
