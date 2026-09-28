use super::fixture::{checked_transform, reference_definition};
use super::{GuiMutationError, ResolvedGuiObject};
use crate::dto::{FixtureStorage, GuiLayoutFixture, GuiLayoutFixtureKind, LayoutGuiEdit};
use donder_language::layout::{FixtureInstanceId, LayoutFixture, LayoutFixtureKind, LayoutId};
use donder_project_io::ProjectSession;

pub(super) fn edit_layout(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: LayoutGuiEdit,
) -> Result<(), GuiMutationError> {
    let id = LayoutId(resolved.identity.clone());
    match edit {
        LayoutGuiEdit::AddDefinition {
            name,
            storage,
            parent,
            transform,
        } => {
            if name.trim().is_empty() {
                return Err(GuiMutationError::Invalid("Enter a fixture name.".into()));
            }
            let identity = match storage {
                FixtureStorage::Inline => session
                    .source
                    .add_object(
                        resolved.identity.document_id(),
                        donder_project_io::SourceObjectKind::FixtureDefinition,
                        "fixture",
                    )
                    .map_err(GuiMutationError::Invalid)?,
                FixtureStorage::NewFile => super::model::create_object_document(
                    session,
                    donder_project_io::SourceObjectKind::FixtureDefinition,
                    &name,
                    "fixtures",
                    "fixture",
                )?,
            };
            let definition = donder_language::fixture::FixtureDefinitionId(identity);
            session.project.definitions.fixtures.definitions.insert(
                definition.clone(),
                donder_language::fixture::FixtureDefinition {
                    elements: Vec::new(),
                },
            );
            donder_project_io::ensure_document_can_reference_source(
                session,
                resolved.identity.document_id(),
                donder_project_io::SourceObjectKind::FixtureDefinition,
                &definition.0,
            )
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
            add_instance(session, &id, name, definition, parent, transform)?;
        }
        LayoutGuiEdit::SetFixtures { fixtures } => {
            let previous = session
                .project
                .layouts
                .get(&id)
                .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?
                .iter_fixtures()
                .filter_map(|fixture| match &fixture.kind {
                    LayoutFixtureKind::Fixture { definition, .. }
                        if session.source.is_project_owned(definition.0.document_id())
                            && session.project.layouts.keys().any(|layout| {
                                layout.0.document_id() == definition.0.document_id()
                            }) =>
                    {
                        Some(definition.clone())
                    }
                    _ => None,
                })
                .collect::<std::collections::HashSet<_>>();
            let fixtures = fixtures
                .into_iter()
                .map(|fixture| domain_fixture(session, &id, fixture))
                .collect::<Result<_, _>>()?;
            session
                .project
                .layouts
                .get_mut(&id)
                .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?
                .fixtures = fixtures;
            for definition in previous {
                let used = session.project.layouts.values().any(|layout| layout.iter_fixtures().any(|fixture|
                    matches!(&fixture.kind, LayoutFixtureKind::Fixture { definition: other, .. } if other == &definition)
                ));
                if !used {
                    session
                        .source
                        .remove_object(
                            &definition.0,
                            donder_project_io::SourceObjectKind::FixtureDefinition,
                        )
                        .map_err(GuiMutationError::Invalid)?;
                    session
                        .project
                        .definitions
                        .fixtures
                        .definitions
                        .shift_remove(&definition);
                }
            }
        }
        LayoutGuiEdit::MoveFixture {
            id: fixture_id,
            delta,
        } => {
            let layout = session
                .project
                .layouts
                .get_mut(&id)
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
            definition: reference_definition(session, &layout.0, definition)?,
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

fn find_fixture_mut(
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
    definition: donder_language::fixture::FixtureDefinitionId,
    parent: Option<u32>,
    transform: crate::dto::Transform,
) -> Result<(), GuiMutationError> {
    let layout = session
        .project
        .layouts
        .get_mut(layout_id)
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
    Ok(())
}
