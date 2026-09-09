use super::{
    GuiMutationError,
    model::{domain_point3_meters, fixture_definition_mut},
};
use crate::dto::{Geometry, Point3Meters, PropGuiEdit};
use dawn_language::identity::SourceIdentity;
use dawn_language::preview::{PropDefinition, PropDefinitionId, PropGeometry};
use dawn_language::values::DistanceSpan;
use dawn_project_io::ProjectSession;

pub(super) fn edit_fixture(
    session: &mut ProjectSession,
    identity: &SourceIdentity,
    edit: PropGuiEdit,
) -> Result<(), GuiMutationError> {
    match edit {
        PropGuiEdit::UpdateDefinition {
            geometry,
            bulb_diameter_meters,
        } => {
            let definition = PropDefinition {
                geometry: domain_geometry(geometry)?,
                bulb_radius: bulb_radius(bulb_diameter_meters)?,
            };
            let changed = dawn_language::preview::authoring::update_definition(
                &mut session.project,
                &PropDefinitionId(identity.clone()),
                definition,
            )
            .map_err(GuiMutationError::Invalid)?;
            for identity in changed {
                super::setup::ensure_owned_target(session, &identity)?;
            }
        }
        PropGuiEdit::MovePoint { point_index, point } => {
            let definition = fixture_definition_mut(session, identity)?;
            let PropGeometry::Points { points } = &mut definition.geometry else {
                return Err(GuiMutationError::Invalid(
                    "Fixture geometry does not contain movable points.".into(),
                ));
            };
            let target = points
                .get_mut(point_index as usize)
                .ok_or_else(|| GuiMutationError::Invalid("Fixture point was not found.".into()))?;
            *target = checked_point(point)?;
        }
    }
    Ok(())
}

pub(crate) fn bulb_radius(diameter: f32) -> Result<DistanceSpan, GuiMutationError> {
    if !diameter.is_finite() || diameter <= 0.0 || diameter > 100.0 {
        return Err(GuiMutationError::Invalid(
            "Bulb diameter must be positive and at most 100 meters.".into(),
        ));
    }
    Ok(DistanceSpan::from_meters(diameter / 2.0))
}

pub(crate) fn checked_point(
    point: Point3Meters,
) -> Result<dawn_language::values::Point3, GuiMutationError> {
    if [point.x_meters, point.y_meters, point.z_meters]
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 2_000.0)
    {
        return Err(GuiMutationError::Invalid(
            "Coordinates must be finite and within 2,000 meters of the origin.".into(),
        ));
    }
    Ok(domain_point3_meters(point))
}

pub(crate) fn domain_geometry(geometry: Geometry) -> Result<PropGeometry, GuiMutationError> {
    let geometry = match geometry {
        Geometry::Points { points } => PropGeometry::Points {
            points: points
                .into_iter()
                .map(checked_point)
                .collect::<Result<_, _>>()?,
        },
        Geometry::Lines { points, pixels } => {
            if points.len() < 2 {
                return Err(GuiMutationError::Invalid(
                    "A line needs at least two points.".into(),
                ));
            }
            PropGeometry::Lines {
                points: points
                    .into_iter()
                    .map(checked_point)
                    .collect::<Result<_, _>>()?,
                point_count: pixels,
            }
        }
        Geometry::Arc {
            center,
            radius_meters,
            start_degrees,
            end_degrees,
            pixels,
        } => {
            if !radius_meters.is_finite()
                || !(0.0..=2_000.0).contains(&radius_meters)
                || radius_meters == 0.0
                || !start_degrees.is_finite()
                || !end_degrees.is_finite()
            {
                return Err(GuiMutationError::Invalid(
                    "An arc needs a positive radius up to 2,000 meters and finite angles.".into(),
                ));
            }
            PropGeometry::Arc {
                center: checked_point(center)?,
                radius: DistanceSpan::from_meters(radius_meters),
                start_degrees,
                end_degrees,
                point_count: pixels,
            }
        }
    };
    if geometry.point_count() == 0 {
        return Err(GuiMutationError::Invalid(
            "A light needs at least one pixel.".into(),
        ));
    }
    Ok(geometry)
}
