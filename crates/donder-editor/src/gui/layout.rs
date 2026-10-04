use super::fixture::{checked_transform, reference_definition};
use super::{GuiMutationError, ResolvedGuiObject};
use crate::dto::{FixtureStorage, GuiLayoutFixture, GuiLayoutFixtureKind, LayoutGuiEdit};
use donder_language::fixture::{FixtureDefinition, FixtureSource};
use donder_language::layout::{FixtureInstanceId, LayoutFixture, LayoutFixtureKind, LayoutId};
use donder_project_io::ProjectSession;

pub(super) fn edit_layout(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: LayoutGuiEdit,
) -> Result<(), GuiMutationError> {
    let id = LayoutId(resolved.object_identity());
    match edit {
        LayoutGuiEdit::ReparentFixture {
            id: fixture_id,
            parent,
            before,
        } => {
            let mut layout = session
                .project
                .layout(&id)
                .cloned()
                .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?;
            layout
                .reparent_fixture(
                    FixtureInstanceId(fixture_id),
                    parent.map(FixtureInstanceId),
                    before.map(FixtureInstanceId),
                )
                .map_err(GuiMutationError::Invalid)?;
            session
                .project
                .replace_layout(&id, layout)
                .map_err(GuiMutationError::Invalid)?;
        }
        LayoutGuiEdit::DuplicateFixture { id: fixture_id } => {
            donder_language::ownership::edit::duplicate_layout_fixture(
                &mut session.project,
                &id,
                FixtureInstanceId(fixture_id),
            )
            .map_err(GuiMutationError::Invalid)?;
        }
        LayoutGuiEdit::RepeatFixtures { ids, offsets } => {
            if ids.is_empty()
                || offsets.is_empty()
                || ids.len().saturating_mul(offsets.len()) > 1000
            {
                return Err(GuiMutationError::Invalid(
                    "Repeat requires a selection and at most 1,000 copies.".into(),
                ));
            }
            let layout = session
                .project
                .layout(&id)
                .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?;
            let mut seen = std::collections::BTreeSet::new();
            fn collect(
                fixture: &LayoutFixture,
                seen: &mut std::collections::BTreeSet<u32>,
            ) -> Result<(), GuiMutationError> {
                if !seen.insert(fixture.id.0) {
                    return Err(GuiMutationError::Invalid(
                        "Select each fixture or group only once, without its descendants.".into(),
                    ));
                }
                if let LayoutFixtureKind::Group { children } = &fixture.kind {
                    for child in children {
                        collect(child, seen)?;
                    }
                }
                Ok(())
            }
            for fixture_id in &ids {
                let fixture = layout
                    .fixture(FixtureInstanceId(*fixture_id))
                    .ok_or_else(|| GuiMutationError::Invalid("Fixture was not found.".into()))?;
                collect(fixture, &mut seen)?;
            }
            // Insert after the original in reverse so the authored copy order matches the array.
            for offset in offsets.into_iter().rev() {
                super::fixture::checked_point(offset.clone())?;
                for fixture_id in &ids {
                    let copy = donder_language::ownership::edit::duplicate_layout_fixture(
                        &mut session.project,
                        &id,
                        FixtureInstanceId(*fixture_id),
                    )
                    .map_err(GuiMutationError::Invalid)?;
                    let mut layout =
                        session.project.layout(&id).cloned().ok_or_else(|| {
                            GuiMutationError::Invalid("Layout was not found.".into())
                        })?;
                    let fixture =
                        find_fixture_mut(&mut layout.fixtures, copy).ok_or_else(|| {
                            GuiMutationError::Invalid("Copied fixture was not found.".into())
                        })?;
                    translate_fixture(fixture, &offset)?;
                    session
                        .project
                        .replace_layout(&id, layout)
                        .map_err(GuiMutationError::Invalid)?;
                }
            }
        }
        LayoutGuiEdit::AddDefinition {
            name,
            storage,
            parent,
            transform,
        } => {
            if name.trim().is_empty() {
                return Err(GuiMutationError::Invalid("Enter a fixture name.".into()));
            }
            let definition = match storage {
                FixtureStorage::Inline => FixtureSource::Inline(FixtureDefinition {
                    elements: Vec::new(),
                }),
                FixtureStorage::SameFile | FixtureStorage::NewFile => {
                    let identity = if matches!(storage, FixtureStorage::SameFile) {
                        session
                            .source
                            .add_object(
                                resolved.identity.document_id(),
                                donder_project_io::SourceObjectKind::FixtureDefinition,
                                &super::model::object_key(&name),
                            )
                            .map_err(GuiMutationError::Invalid)?
                    } else {
                        super::model::create_object_document(
                            session,
                            donder_project_io::SourceObjectKind::FixtureDefinition,
                            &name,
                            "fixtures",
                            "fixture",
                        )?
                    };
                    let definition = donder_language::fixture::FixtureDefinitionId(identity);
                    session
                        .project
                        .apply_edits([donder_language::model::ProjectEdit::SetFixtureDefinition {
                            id: definition.clone(),
                            value: FixtureDefinition {
                                elements: Vec::new(),
                            },
                        }])
                        .map_err(GuiMutationError::Invalid)?;
                    donder_project_io::ensure_document_can_reference_source(
                        session,
                        resolved.identity.document_id(),
                        donder_project_io::SourceObjectKind::FixtureDefinition,
                        &definition.0,
                    )
                    .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
                    FixtureSource::Reference(definition)
                }
            };
            add_instance(session, &id, name, definition, parent, transform)?;
        }
        LayoutGuiEdit::SetFixtures { fixtures } => {
            let fixtures = fixtures
                .into_iter()
                .map(|fixture| domain_fixture(session, &id, fixture))
                .collect::<Result<_, _>>()?;
            let mut layout = session
                .project
                .layout(&id)
                .cloned()
                .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?;
            layout.fixtures = fixtures;
            session
                .project
                .replace_layout(&id, layout)
                .map_err(GuiMutationError::Invalid)?;
        }

        LayoutGuiEdit::MoveFixture {
            id: fixture_id,
            delta,
        } => {
            let mut layout = session
                .project
                .layout(&id)
                .cloned()
                .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?;
            let fixture = find_fixture_mut(&mut layout.fixtures, FixtureInstanceId(fixture_id))
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Fixture instance was not found.".into())
                })?;
            let LayoutFixtureKind::Fixture {
                transform: current, ..
            } = &mut fixture.kind
            else {
                return Err(GuiMutationError::Invalid(
                    "Select a fixture instance to edit its transform.".into(),
                ));
            };
            current.position = super::fixture::checked_point(crate::dto::Point3Meters {
                x_meters: current.position.x.as_meters_f32() + delta.x_meters,
                y_meters: current.position.y.as_meters_f32() + delta.y_meters,
                z_meters: current.position.z.as_meters_f32() + delta.z_meters,
            })?;
            session
                .project
                .replace_layout(&id, layout)
                .map_err(GuiMutationError::Invalid)?;
        }
    }
    Ok(())
}

fn domain_fixture(
    session: &mut ProjectSession,
    layout: &LayoutId,
    fixture: GuiLayoutFixture,
) -> Result<LayoutFixture, GuiMutationError> {
    let kind = match fixture.kind {
        GuiLayoutFixtureKind::Fixture {
            definition,
            transform,
        } => LayoutFixtureKind::Fixture {
            definition: match definition {
                crate::dto::GuiFixtureSource::Inline { elements } => {
                    FixtureSource::Inline(super::fixture::domain_geometry(elements)?)
                }
                crate::dto::GuiFixtureSource::Reference { source } => FixtureSource::Reference(
                    reference_definition(session, layout.0.root_source(), source)?,
                ),
            },
            transform: checked_transform(transform)?,
        },
        GuiLayoutFixtureKind::Group { children } => LayoutFixtureKind::Group {
            children: children
                .into_iter()
                .map(|child| domain_fixture(session, layout, child))
                .collect::<Result<_, _>>()?,
        },
    };
    Ok(LayoutFixture {
        id: FixtureInstanceId(fixture.id),
        name: fixture.name,
        kind,
    })
}

pub(super) fn find_fixture_mut(
    fixtures: &mut [LayoutFixture],
    id: FixtureInstanceId,
) -> Option<&mut LayoutFixture> {
    for fixture in fixtures {
        if fixture.id == id {
            return Some(fixture);
        }
        if let LayoutFixtureKind::Group { children } = &mut fixture.kind
            && let Some(fixture) = find_fixture_mut(children, id)
        {
            return Some(fixture);
        }
    }
    None
}
fn add_instance(
    session: &mut ProjectSession,
    layout_id: &LayoutId,
    name: String,
    definition: FixtureSource,
    parent: Option<u32>,
    transform: crate::dto::Transform,
) -> Result<(), GuiMutationError> {
    let mut layout = session
        .project
        .layout(layout_id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?;
    let id = layout
        .iter_fixtures()
        .map(|fixture| fixture.id.0)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| GuiMutationError::Invalid("No fixture instance IDs remain.".into()))?;
    let children = match parent {
        None => &mut layout.fixtures,
        Some(parent) => {
            let group = find_fixture_mut(&mut layout.fixtures, FixtureInstanceId(parent))
                .ok_or_else(|| GuiMutationError::Invalid("Group was not found.".into()))?;
            let LayoutFixtureKind::Group { children } = &mut group.kind else {
                return Err(GuiMutationError::Invalid(
                    "Fixtures can only be added to groups.".into(),
                ));
            };
            children
        }
    };
    children.push(LayoutFixture {
        id: FixtureInstanceId(id),
        name,
        kind: LayoutFixtureKind::Fixture {
            definition,
            transform: checked_transform(transform)?,
        },
    });
    session
        .project
        .replace_layout(layout_id, layout)
        .map_err(GuiMutationError::Invalid)
}

fn translate_fixture(
    fixture: &mut LayoutFixture,
    delta: &crate::dto::Point3Meters,
) -> Result<(), GuiMutationError> {
    match &mut fixture.kind {
        LayoutFixtureKind::Fixture { transform, .. } => {
            transform.position = super::fixture::checked_point(crate::dto::Point3Meters {
                x_meters: transform.position.x.as_meters_f32() + delta.x_meters,
                y_meters: transform.position.y.as_meters_f32() + delta.y_meters,
                z_meters: transform.position.z.as_meters_f32() + delta.z_meters,
            })?;
        }
        LayoutFixtureKind::Group { children } => {
            for child in children {
                translate_fixture(child, delta)?;
            }
        }
    }
    Ok(())
}
