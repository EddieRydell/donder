//! The runtime geometry contract is established with the authored layout, not
//! rediscovered while preparing a selected sequence.
use super::DonderProject;
use crate::layout::{FixtureId, LayoutId};
use crate::validation::{AdmissionProblem, ProjectValidationError, ValidationProblem, at};
use donder_runtime_types::FixtureGeometry;
use indexmap::IndexMap;
use std::sync::Arc;

pub(super) type LayoutGeometry = Arc<[(FixtureId, FixtureGeometry)]>;

pub(super) fn admit(
    project: &DonderProject,
    previous: Option<&DonderProject>,
) -> Result<IndexMap<LayoutId, LayoutGeometry>, ProjectValidationError> {
    let previous = previous.filter(|previous| {
        let fixtures = project.reusable::<crate::FixtureDefinition>();
        let previous_fixtures = previous.reusable::<crate::FixtureDefinition>();
        std::ptr::eq(fixtures, previous_fixtures) || fixtures == previous_fixtures
    });
    // Sequence-only edits need neither definition expansion nor transformed
    // coordinates. Prepare definitions lazily, once a layout needs new geometry.
    let mut definitions = None;
    project
        .objects::<crate::Layout>()
        .map(|(id, layout)| {
            if let Some(previous) = previous
                && previous
                    .get::<crate::Layout>(id)
                    .is_some_and(|previous_layout| {
                        std::ptr::eq(layout, previous_layout) || layout == previous_layout
                    })
            {
                return Ok((id, Arc::clone(&previous.accepted_inputs.layouts[&id])));
            }
            let definitions = definitions.get_or_insert_with(|| {
                crate::geometry::PreparedFixtureDefinitions::prepare(
                    project.reusable::<crate::FixtureDefinition>(),
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
                            instance
                                .transform
                                .transform_point3(pixel.position)
                                .to_array()
                        })
                        .collect();
                    let strands = instance
                        .pixels
                        .iter()
                        .map(|pixel| (pixel.strand, pixel.along))
                        .collect();
                    let geometry = FixtureGeometry::admit(positions, strands).ok_or(
                        ValidationProblem::Admission(AdmissionProblem::GeometryOutOfRange),
                    )?;
                    Ok((instance.id, geometry))
                })
                .collect::<Result<LayoutGeometry, ValidationProblem>>()
                .map_err(at(id))?;
            Ok((id, fixtures))
        })
        .collect()
}
