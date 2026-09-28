use super::{
    GuiMutationError,
    model::{
        domain_point3_meters, fixture_definition_mut, rotation3_degrees, scale3,
        source_identity_from_gui,
    },
};
use crate::dto::*;
use donder_language::fixture::*;
use donder_language::identity::SourceIdentity;
use donder_language::values::DistanceSpan;
use donder_project_io::{ProjectSession, SourceObjectKind};

pub(super) fn edit_fixture(
    session: &mut ProjectSession,
    resolved: &super::ResolvedGuiObject,
    edit: FixtureGuiEdit,
) -> Result<(), GuiMutationError> {
    let fixture = match resolved.owned_path.as_slice() {
        [] => fixture_definition_mut(session, &resolved.identity)?,
        [parent @ .., crate::dto::GuiOwnedStep::Fixture { id }] => {
            let parent = parent.iter().fold(
                donder_language::identity::ObjectIdentity::from(resolved.identity.clone()),
                |address, step| address.owned(step.into()),
            );
            let layout = session
                .project
                .layout_mut(&donder_language::layout::LayoutId(parent))
                .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?;
            let placement = super::layout::find_fixture_mut(
                &mut layout.fixtures,
                donder_language::layout::FixtureInstanceId(*id),
            )
            .ok_or_else(|| GuiMutationError::Invalid("Fixture was not found.".into()))?;
            let donder_language::layout::LayoutFixtureKind::Fixture {
                definition: FixtureSource::Inline(value),
                ..
            } = &mut placement.kind
            else {
                return Err(GuiMutationError::Invalid(
                    "Fixture is not owned inline.".into(),
                ));
            };
            value
        }
        _ => {
            return Err(GuiMutationError::Invalid(
                "Unsupported owned fixture path.".into(),
            ));
        }
    };
    match edit {
        FixtureGuiEdit::SetElements { elements } => {
            fixture.elements = elements
                .into_iter()
                .map(domain_element)
                .collect::<Result<_, _>>()?;
        }
        FixtureGuiEdit::MoveElement { id, delta } => {
            let element = fixture
                .elements
                .iter_mut()
                .find(|element| element.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Shape was not found.".into()))?;
            element.transform.position = checked_point(Point3Meters {
                x_meters: element.transform.position.x.as_meters_f32() + delta.x_meters,
                y_meters: element.transform.position.y.as_meters_f32() + delta.y_meters,
                z_meters: element.transform.position.z.as_meters_f32() + delta.z_meters,
            })?;
        }
        FixtureGuiEdit::MoveHandle {
            id,
            index,
            position,
        } => {
            let element = fixture
                .elements
                .iter_mut()
                .find(|element| element.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Shape was not found.".into()))?;
            move_handle(element, index, position)?;
        }
        FixtureGuiEdit::ConvertToPixels { id } => {
            let index = fixture
                .elements
                .iter()
                .position(|element| element.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Shape was not found.".into()))?;
            let element = &fixture.elements[index];
            let mut next = fixture
                .elements
                .iter()
                .map(|element| element.id.0)
                .max()
                .unwrap_or(0);
            let pixels = donder_elaboration::fixture::element_pixels(element)
                .map_err(|error| {
                    GuiMutationError::Invalid(format!("Cannot expand shape: {error:?}"))
                })?
                .into_iter()
                .enumerate()
                .map(|(ordinal, pixel)| {
                    next = next
                        .checked_add(1)
                        .ok_or_else(|| GuiMutationError::Invalid("No shape IDs remain.".into()))?;
                    Ok(FixtureElement {
                        id: FixtureElementId(next),
                        name: format!("{} {}", element.name, ordinal + 1),
                        transform: FixtureTransform {
                            position: checked_point(Point3Meters {
                                x_meters: pixel.position.x,
                                y_meters: pixel.position.y,
                                z_meters: pixel.position.z,
                            })?,
                            ..Default::default()
                        },
                        diameter: element.diameter,
                        reverse: false,
                        shape: FixtureShape::Pixel,
                    })
                })
                .collect::<Result<Vec<_>, GuiMutationError>>()?;
            fixture.elements.splice(index..=index, pixels);
        }
    }
    validate_geometry(fixture)?;
    Ok(())
}

pub(super) fn domain_geometry(
    elements: Vec<GuiFixtureElement>,
) -> Result<FixtureDefinition, GuiMutationError> {
    let geometry = FixtureDefinition {
        elements: elements
            .into_iter()
            .map(domain_element)
            .collect::<Result<_, _>>()?,
    };
    validate_geometry(&geometry)?;
    Ok(geometry)
}

fn validate_geometry(geometry: &FixtureDefinition) -> Result<(), GuiMutationError> {
    donder_elaboration::fixture::prepare_geometry(geometry).map_err(|error| {
        GuiMutationError::Invalid(format!("Invalid fixture geometry: {error:?}"))
    })?;
    Ok(())
}

pub(super) fn domain_element(
    element: GuiFixtureElement,
) -> Result<FixtureElement, GuiMutationError> {
    let shape = match element.shape {
        GuiFixtureShape::Pixel => FixtureShape::Pixel,
        GuiFixtureShape::Line { length, count } => FixtureShape::Line { length, count },
        GuiFixtureShape::Polyline { points, count } => FixtureShape::Polyline {
            points: points
                .into_iter()
                .map(checked_point)
                .collect::<Result<_, _>>()?,
            count,
        },
        GuiFixtureShape::Arc {
            radius,
            start_degrees,
            sweep_degrees,
            count,
            closed,
        } => FixtureShape::Arc {
            radius,
            start_degrees,
            sweep_degrees,
            count,
            closed,
        },
        GuiFixtureShape::Grid {
            columns,
            rows,
            width,
            height,
            axis,
            corner,
            serpentine,
        } => FixtureShape::Grid {
            columns,
            rows,
            width,
            height,
            axis: match axis {
                GuiGridAxis::Rows => GridAxis::Rows,
                GuiGridAxis::Columns => GridAxis::Columns,
            },
            corner: match corner {
                GuiGridCorner::BottomLeft => GridCorner::BottomLeft,
                GuiGridCorner::BottomRight => GridCorner::BottomRight,
                GuiGridCorner::TopLeft => GridCorner::TopLeft,
                GuiGridCorner::TopRight => GridCorner::TopRight,
            },
            serpentine,
        },
    };
    let element = FixtureElement {
        id: FixtureElementId(element.id),
        name: element.name,
        transform: checked_transform(element.transform)?,
        diameter: pixel_diameter(element.diameter_meters)?,
        reverse: element.reverse,
        shape,
    };
    if !element.is_valid() {
        return Err(GuiMutationError::Invalid(
            "Invalid shape geometry, name, or pixel count.".into(),
        ));
    }
    Ok(element)
}

pub(crate) fn gui_element(element: &FixtureElement) -> GuiFixtureElement {
    let shape = match &element.shape {
        FixtureShape::Pixel => GuiFixtureShape::Pixel,
        FixtureShape::Line { length, count } => GuiFixtureShape::Line {
            length: *length,
            count: *count,
        },
        FixtureShape::Polyline { points, count } => GuiFixtureShape::Polyline {
            points: points
                .iter()
                .map(|point| crate::preview::point3_meters(*point))
                .collect(),
            count: *count,
        },
        FixtureShape::Arc {
            radius,
            start_degrees,
            sweep_degrees,
            count,
            closed,
        } => GuiFixtureShape::Arc {
            radius: *radius,
            start_degrees: *start_degrees,
            sweep_degrees: *sweep_degrees,
            count: *count,
            closed: *closed,
        },
        FixtureShape::Grid {
            columns,
            rows,
            width,
            height,
            axis,
            corner,
            serpentine,
        } => GuiFixtureShape::Grid {
            columns: *columns,
            rows: *rows,
            width: *width,
            height: *height,
            axis: match axis {
                GridAxis::Rows => GuiGridAxis::Rows,
                GridAxis::Columns => GuiGridAxis::Columns,
            },
            corner: match corner {
                GridCorner::BottomLeft => GuiGridCorner::BottomLeft,
                GridCorner::BottomRight => GuiGridCorner::BottomRight,
                GridCorner::TopLeft => GuiGridCorner::TopLeft,
                GridCorner::TopRight => GuiGridCorner::TopRight,
            },
            serpentine: *serpentine,
        },
    };
    GuiFixtureElement {
        id: element.id.0,
        name: element.name.clone(),
        transform: gui_transform(&element.transform),
        diameter_meters: element.diameter.as_meters_f32(),
        reverse: element.reverse,
        shape,
    }
}

pub(crate) fn gui_transform(value: &FixtureTransform) -> Transform {
    Transform {
        position: crate::preview::point3_meters(value.position),
        rotation: Rotation3Degrees {
            x_degrees: value.rotation.x,
            y_degrees: value.rotation.y,
            z_degrees: value.rotation.z,
        },
        scale: Scale3 {
            x: value.scale.x,
            y: value.scale.y,
            z: value.scale.z,
        },
    }
}

fn move_handle(
    element: &mut FixtureElement,
    index: u32,
    position: Point3Meters,
) -> Result<(), GuiMutationError> {
    let position = checked_point(position)?;
    let to_vec = |point: donder_language::values::Point3| {
        glam::Vec3::new(
            point.x.as_meters_f32(),
            point.y.as_meters_f32(),
            point.z.as_meters_f32(),
        )
    };
    if let FixtureShape::Line { length, .. } = &mut element.shape {
        if index > 1 {
            return Err(GuiMutationError::Invalid(
                "Control point was not found.".into(),
            ));
        }
        let transform = donder_elaboration::fixture::fixture_transform(&element.transform);
        let start = if index == 0 {
            to_vec(position)
        } else {
            to_vec(element.transform.position)
        };
        let end = if index == 1 {
            to_vec(position)
        } else {
            transform.transform_point3(glam::Vec3::X * *length)
        };
        let direction = end - start;
        if direction.length_squared() == 0.0 {
            return Err(GuiMutationError::Invalid(
                "Line endpoints must be distinct.".into(),
            ));
        }
        let rotation = glam::Quat::from_euler(
            glam::EulerRot::XYZ,
            element.transform.rotation.x.to_radians(),
            element.transform.rotation.y.to_radians(),
            element.transform.rotation.z.to_radians(),
        );
        let axis = direction.normalize() * element.transform.scale.x.signum();
        let rotation = glam::Quat::from_rotation_arc(rotation * glam::Vec3::X, axis) * rotation;
        let (x, y, z) = rotation.to_euler(glam::EulerRot::XYZ);
        element.transform.rotation.x = x.to_degrees();
        element.transform.rotation.y = y.to_degrees();
        element.transform.rotation.z = z.to_degrees();
        *length = direction.length() / element.transform.scale.x.abs();
        if index == 0 {
            element.transform.position = position;
        }
        return Ok(());
    }
    if index == 0 && !matches!(element.shape, FixtureShape::Polyline { .. }) {
        element.transform.position = position;
        return Ok(());
    }
    let transform = donder_elaboration::fixture::fixture_transform(&element.transform);
    let local = transform.inverse().transform_point3(to_vec(position));
    match &mut element.shape {
        FixtureShape::Polyline { points, .. } => {
            *points.get_mut(index as usize).ok_or_else(|| {
                GuiMutationError::Invalid("Control point was not found.".into())
            })? = checked_point(Point3Meters {
                x_meters: local.x,
                y_meters: local.y,
                z_meters: local.z,
            })?;
        }
        FixtureShape::Arc {
            radius,
            start_degrees,
            sweep_degrees,
            closed,
            ..
        } if index == 1 || (index == 2 && !*closed) => {
            let angle = local.y.atan2(local.x).to_degrees();
            if index == 1 {
                *radius = local.truncate().length();
                *start_degrees = angle;
            } else {
                *sweep_degrees = if *sweep_degrees > 0.0 {
                    (angle - *start_degrees).rem_euclid(360.0)
                } else {
                    -(*start_degrees - angle).rem_euclid(360.0)
                };
            }
        }
        FixtureShape::Grid { width, height, .. } if index == 1 => {
            *width = local.x;
            *height = local.y;
        }
        _ => {
            return Err(GuiMutationError::Invalid(
                "Control point was not found.".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn reference_definition(
    session: &mut ProjectSession,
    owner: &SourceIdentity,
    reference: GuiObjectRef,
) -> Result<FixtureDefinitionId, GuiMutationError> {
    if !reference.owned_path.is_empty() || !matches!(reference.kind, ObjectKind::Fixture) {
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
    donder_project_io::link_reusable_source(
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
