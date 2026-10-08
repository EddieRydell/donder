use super::fixture::{checked_transform, reference_definition};
use super::{GuiMutationError, ResolvedGuiObject};
use donder_model::{FixtureDefinition, FixtureSource};
use donder_model::{FixtureInstanceId, Layout, LayoutFixture, LayoutFixtureKind, LayoutId};
use donder_project_io::ProjectSession;
use donder_sequence_api::{FixtureStorage, GuiLayoutFixture, GuiLayoutFixtureKind, LayoutGuiEdit};

pub(super) fn edit_layout(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: LayoutGuiEdit,
) -> Result<(), GuiMutationError> {
    let id = LayoutId(resolved.object_identity());
    match edit {
        LayoutGuiEdit::MoveMember {
            id: member,
            from,
            to,
            before,
        } => update_layout(session, &id, |layout| {
            layout.move_member(
                FixtureInstanceId(member),
                from.map(FixtureInstanceId),
                to.map(FixtureInstanceId),
                before.map(FixtureInstanceId),
            )
        })?,
        LayoutGuiEdit::AddMember {
            id: member,
            to,
            before,
        } => update_layout(session, &id, |layout| {
            layout.add_member(
                FixtureInstanceId(member),
                to.map(FixtureInstanceId),
                before.map(FixtureInstanceId),
            )
        })?,
        LayoutGuiEdit::RemoveMember { id: member, from } => {
            update_layout(session, &id, |layout| {
                layout.remove_member(FixtureInstanceId(member), from.map(FixtureInstanceId))
            })?
        }
        LayoutGuiEdit::RemoveItems { ids } => update_layout(session, &id, |layout| {
            layout.remove_items(&ids.into_iter().map(FixtureInstanceId).collect::<Vec<_>>())
        })?,
        LayoutGuiEdit::DuplicateFixture { id: fixture_id } => {
            donder_model::duplicate_layout_fixture(
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
            // Copies of overlapping selections would duplicate a fixture twice.
            let mut seen = std::collections::BTreeSet::new();
            for fixture_id in &ids {
                let fixture = FixtureInstanceId(*fixture_id);
                if layout.fixture(fixture).is_none() {
                    return Err(GuiMutationError::Invalid("Fixture was not found.".into()));
                }
                let subtree = layout
                    .iter_fixtures()
                    .filter(|item| layout.contains(fixture, item.id))
                    .map(|item| item.id);
                for item in subtree {
                    if !seen.insert(item) {
                        return Err(GuiMutationError::Invalid(
                            "Select each fixture or group only once, without its descendants."
                                .into(),
                        ));
                    }
                }
            }
            // Insert after the original in reverse so the authored copy order matches the array.
            for offset in offsets.into_iter().rev() {
                super::fixture::checked_point(offset.clone())?;
                for fixture_id in &ids {
                    let copy = donder_model::duplicate_layout_fixture(
                        &mut session.project,
                        &id,
                        FixtureInstanceId(*fixture_id),
                    )
                    .map_err(GuiMutationError::Invalid)?;
                    let mut layout =
                        session.project.layout(&id).cloned().ok_or_else(|| {
                            GuiMutationError::Invalid("Layout was not found.".into())
                        })?;
                    for member in layout.members(copy) {
                        let fixture = layout.fixture_mut(member).ok_or_else(|| {
                            GuiMutationError::Invalid("Copied fixture was not found.".into())
                        })?;
                        translate_fixture(fixture, &offset)?;
                    }
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
                    description: None,
                    elements: Vec::new(),
                }),
                FixtureStorage::SameFile | FixtureStorage::NewFile => {
                    let identity = if matches!(storage, FixtureStorage::SameFile) {
                        session
                            .source
                            .add_object(
                                resolved.identity.document_id(),
                                donder_project_io::SourceObjectKind::FixtureDefinition,
                                donder_language::object_name(&name).as_str(),
                            )
                            .map_err(GuiMutationError::Invalid)?
                    } else {
                        super::model::create_object_document(
                            session,
                            donder_project_io::SourceObjectKind::FixtureDefinition,
                            &name,
                            "fixtures",
                        )?
                    };
                    let definition = donder_model::FixtureDefinitionId(identity);
                    session
                        .project
                        .apply_edits([donder_model::ProjectEdit::SetFixtureDefinition {
                            id: definition.clone(),
                            value: FixtureDefinition {
                                description: None,
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
        LayoutGuiEdit::SetFixtures { fixtures, root } => {
            let fixtures = fixtures
                .into_iter()
                .map(|fixture| domain_fixture(session, &id, fixture))
                .collect::<Result<_, _>>()?;
            update_layout(session, &id, |layout| {
                layout.fixtures = fixtures;
                layout.root = root.into_iter().map(FixtureInstanceId).collect();
                Ok(())
            })?;
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
            let fixture = layout
                .fixture_mut(FixtureInstanceId(fixture_id))
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Fixture instance was not found.".into())
                })?;
            if !matches!(fixture.kind, LayoutFixtureKind::Fixture { .. }) {
                return Err(GuiMutationError::Invalid(
                    "Select a fixture instance to edit its transform.".into(),
                ));
            }
            translate_fixture(fixture, &delta)?;
            session
                .project
                .replace_layout(&id, layout)
                .map_err(GuiMutationError::Invalid)?;
        }
    }
    Ok(())
}

fn update_layout(
    session: &mut ProjectSession,
    id: &LayoutId,
    update: impl FnOnce(&mut Layout) -> Result<(), String>,
) -> Result<(), GuiMutationError> {
    let mut layout = session
        .project
        .layout(id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?;
    update(&mut layout).map_err(GuiMutationError::Invalid)?;
    session
        .project
        .replace_layout(id, layout)
        .map_err(GuiMutationError::Invalid)
}

fn domain_fixture(
    session: &mut ProjectSession,
    layout: &LayoutId,
    fixture: GuiLayoutFixture,
) -> Result<LayoutFixture, GuiMutationError> {
    // The layout view edits shapes of an owned definition, not its description.
    let owned_description = session
        .project
        .layout(layout)
        .and_then(|current| current.fixture(FixtureInstanceId(fixture.id)))
        .and_then(|current| match &current.kind {
            LayoutFixtureKind::Fixture {
                definition: FixtureSource::Inline(definition),
                ..
            } => definition.description.clone(),
            _ => None,
        });
    let kind = match fixture.kind {
        GuiLayoutFixtureKind::Fixture {
            definition,
            transform,
        } => LayoutFixtureKind::Fixture {
            definition: match definition {
                donder_sequence_api::GuiFixtureSource::Inline { elements } => {
                    let mut definition = super::fixture::domain_geometry(elements)?;
                    definition.description = owned_description;
                    FixtureSource::Inline(definition)
                }
                donder_sequence_api::GuiFixtureSource::Reference { source } => {
                    FixtureSource::Reference(reference_definition(
                        session,
                        layout.0.root_source(),
                        source,
                    )?)
                }
            },
            transform: checked_transform(transform)?,
        },
        GuiLayoutFixtureKind::Group { members } => LayoutFixtureKind::Group {
            members: members.into_iter().map(FixtureInstanceId).collect(),
        },
    };
    Ok(LayoutFixture {
        id: FixtureInstanceId(fixture.id),
        name: super::model::typed_name(&fixture.name)?,
        description: super::description::normalized(fixture.description),
        kind,
    })
}

fn add_instance(
    session: &mut ProjectSession,
    layout_id: &LayoutId,
    name: String,
    definition: FixtureSource,
    parent: Option<u32>,
    transform: donder_sequence_api::Transform,
) -> Result<(), GuiMutationError> {
    let name = super::model::typed_name(&name)?;
    let transform = checked_transform(transform)?;
    update_layout(session, layout_id, |layout| {
        let id = layout
            .iter_fixtures()
            .map(|fixture| fixture.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .map(FixtureInstanceId)
            .ok_or("No fixture instance IDs remain.")?;
        layout.fixtures.push(LayoutFixture {
            id,
            name,
            description: None,
            kind: LayoutFixtureKind::Fixture {
                definition,
                transform,
            },
        });
        layout.add_member(id, parent.map(FixtureInstanceId), None)
    })
}

fn translate_fixture(
    fixture: &mut LayoutFixture,
    delta: &donder_sequence_api::Point3Meters,
) -> Result<(), GuiMutationError> {
    if let LayoutFixtureKind::Fixture { transform, .. } = &mut fixture.kind {
        transform.position = super::fixture::checked_point(donder_sequence_api::Point3Meters {
            x_meters: transform.position.x.as_meters_f32() + delta.x_meters,
            y_meters: transform.position.y.as_meters_f32() + delta.y_meters,
            z_meters: transform.position.z.as_meters_f32() + delta.z_meters,
        })?;
    }
    Ok(())
}
