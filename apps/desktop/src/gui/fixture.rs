use donder_language::fixture::{FixtureDefinitionId, FixtureTransform, Pixel, PixelId};
use donder_language::identity::SourceIdentity;
use donder_language::values::DistanceSpan;
use donder_project_io::{ProjectSession, SourceObjectKind, ensure_document_can_reference_source};

use super::{
    GuiMutationError,
    model::{
        domain_point3_meters, fixture_definition_mut, rotation3_degrees, scale3,
        source_identity_from_gui,
    },
};
use crate::dto::{FixtureGuiEdit, GuiObjectRef, GuiPixel, ObjectKind, Point3Meters, Transform};

pub(super) fn edit_fixture(
    session: &mut ProjectSession,
    identity: &SourceIdentity,
    edit: FixtureGuiEdit,
) -> Result<(), GuiMutationError> {
    match edit {
        FixtureGuiEdit::SetPixels { pixels } => {
            fixture_definition_mut(session, identity)?.pixels = pixels
                .into_iter()
                .map(domain_pixel)
                .collect::<Result<_, _>>()?;
        }
        FixtureGuiEdit::MovePixel { id, delta } => {
            let pixel = fixture_definition_mut(session, identity)?
                .pixels
                .iter_mut()
                .find(|pixel| pixel.id == PixelId(id))
                .ok_or_else(|| GuiMutationError::Invalid("Pixel was not found.".into()))?;
            pixel.position = checked_point(Point3Meters {
                x_meters: pixel.position.x.as_meters_f32() + delta.x_meters,
                y_meters: pixel.position.y.as_meters_f32() + delta.y_meters,
                z_meters: pixel.position.z.as_meters_f32() + delta.z_meters,
            })?;
        }
    }
    Ok(())
}

fn domain_pixel(pixel: GuiPixel) -> Result<Pixel, GuiMutationError> {
    Ok(Pixel {
        id: PixelId(pixel.id),
        position: checked_point(pixel.position)?,
        diameter: pixel_diameter(pixel.diameter_meters)?,
    })
}

pub(super) fn reference_definition(
    session: &mut ProjectSession,
    owner: &SourceIdentity,
    reference: GuiObjectRef,
) -> Result<FixtureDefinitionId, GuiMutationError> {
    if !matches!(reference.kind, ObjectKind::Fixture) {
        return Err(GuiMutationError::Invalid(
            "Choose a fixture definition.".into(),
        ));
    }
    let identity =
        source_identity_from_gui(&reference.module_id, &reference.path, &reference.object_key)?;
    let id = FixtureDefinitionId(identity);
    if !session
        .project
        .definitions
        .fixtures
        .definitions
        .contains_key(&id)
    {
        return Err(GuiMutationError::Invalid(
            "Fixture definition was not found.".into(),
        ));
    }
    ensure_document_can_reference_source(
        session,
        owner.document_id(),
        SourceObjectKind::FixtureDefinition,
        &id.0,
    )
    .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    Ok(id)
}

pub(super) fn checked_transform(
    transform: Transform,
) -> Result<FixtureTransform, GuiMutationError> {
    let result = FixtureTransform {
        position: checked_point(transform.position)?,
        rotation: rotation3_degrees(transform.rotation),
        scale: scale3(transform.scale),
    };
    if !result.is_valid() {
        return Err(GuiMutationError::Invalid(
            "Rotation and scale must be finite; scale cannot be zero.".into(),
        ));
    }
    Ok(result)
}

fn pixel_diameter(diameter: f32) -> Result<DistanceSpan, GuiMutationError> {
    if !diameter.is_finite() || diameter < 0.000001 || diameter > 100.0 {
        return Err(GuiMutationError::Invalid(
            "Pixel diameter must be positive and at most 100 meters.".into(),
        ));
    }
    Ok(DistanceSpan::from_meters(diameter))
}

pub(crate) fn checked_point(
    point: Point3Meters,
) -> Result<donder_language::values::Point3, GuiMutationError> {
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
