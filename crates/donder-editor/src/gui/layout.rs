use super::fixture::{checked_transform, reference_definition};
use super::{GuiMutationError, ResolvedGuiObject, next_id};
use crate::dto::{GuiLayoutFixture, GuiLayoutFixtureKind, LayoutGuiEdit};
use donder_model::{FixtureDefinition, OwnedFixture};
use donder_model::{FixtureInstanceId, Layout, LayoutFixture, LayoutFixtureKind, LayoutId};
use donder_project_io::ProjectSession;

pub(super) fn edit_layout(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: LayoutGuiEdit,
) -> Result<(), GuiMutationError> {
    let id = resolved.id::<LayoutId>();
    match edit {
        LayoutGuiEdit::MoveMember {
            id: member,
            from,
            to,
            before,
        } => update_layout(session, resolved, |layout| {
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
        } => update_layout(session, resolved, |layout| {
            layout.add_member(
                FixtureInstanceId(member),
                to.map(FixtureInstanceId),
                before.map(FixtureInstanceId),
            )
        })?,
        LayoutGuiEdit::RemoveMember { id: member, from } => {
            update_layout(session, resolved, |layout| {
                layout.remove_member(FixtureInstanceId(member), from.map(FixtureInstanceId))
            })?
        }
        LayoutGuiEdit::RemoveItems { ids } => update_layout(session, resolved, |layout| {
            layout.remove_items(&ids.into_iter().map(FixtureInstanceId).collect::<Vec<_>>())
        })?,
        LayoutGuiEdit::DuplicateFixture { id: fixture_id } => {
            session.edit_project(|project| {
                donder_model::duplicate_layout_fixture(project, &id, FixtureInstanceId(fixture_id))
                    .map_err(GuiMutationError::from)
            })?;
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
            let layout = resolved.view::<Layout>(session);
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
                    let copy = session.edit_project(|project| {
                        donder_model::duplicate_layout_fixture(
                            project,
                            &id,
                            FixtureInstanceId(*fixture_id),
                        )
                        .map_err(GuiMutationError::from)
                    })?;
                    let mut layout = Layout::clone(&resolved.view(session));
                    let mut moved = Vec::new();
                    for part in layout.members(copy) {
                        if moved.contains(&part.fixture) {
                            continue;
                        }
                        moved.push(part.fixture);
                        let Some(fixture) = layout.fixture_mut(part.fixture) else {
                            unreachable!("a copy's members are in its layout");
                        };
                        translate_fixture(fixture, &offset)?;
                    }
                    session.put(layout).map_err(GuiMutationError::from)?;
                }
            }
        }
        LayoutGuiEdit::AddDefinition {
            name,
            parent,
            transform,
        } => {
            let definition = donder_model::FixtureDefinitionId(session.new_id());
            let declared = super::model::typed_name(&name)?;
            session.edit(|edit| {
                edit.add_object_document(definition.into(), declared)?;
                edit.project_mut()
                    .put(donder_model::Object::FixtureDefinition(
                        definition,
                        FixtureDefinition {
                            description: None,
                            elements: Vec::new(),
                        },
                    ))?;
                Ok::<_, GuiMutationError>(())
            })?;
            let definition = OwnedFixture::Reference(definition);
            add_instance(session, resolved, name, definition, parent, transform)?;
        }
        LayoutGuiEdit::SetFixtures { fixtures, root } => {
            let fixtures = fixtures
                .into_iter()
                .map(|fixture| domain_fixture(session, resolved, fixture))
                .collect::<Result<_, _>>()?;
            update_layout(session, resolved, |layout| {
                layout.fixtures = fixtures;
                layout.root = root.into_iter().map(FixtureInstanceId).collect();
                Ok(())
            })?;
        }

        LayoutGuiEdit::MoveFixture {
            id: fixture_id,
            delta,
        } => {
            let mut layout = Layout::clone(&resolved.view(session));
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
            session.put(layout).map_err(GuiMutationError::from)?;
        }
    }
    Ok(())
}

fn update_layout(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    update: impl FnOnce(&mut Layout) -> Result<(), donder_model::EditRejection>,
) -> Result<(), GuiMutationError> {
    let mut layout = Layout::clone(&resolved.view(session));
    update(&mut layout)?;
    session.put(layout).map_err(GuiMutationError::from)
}

fn domain_fixture(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    fixture: GuiLayoutFixture,
) -> Result<LayoutFixture, GuiMutationError> {
    // The layout view edits shapes of an owned definition, not its description.
    let owned_description = resolved
        .view::<Layout>(session)
        .fixture(FixtureInstanceId(fixture.id))
        .and_then(|current| match &current.kind {
            LayoutFixtureKind::Fixture {
                definition: OwnedFixture::Inline(definition),
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
                crate::dto::GuiOwnedFixture::Inline { elements } => {
                    let mut definition = super::fixture::domain_geometry(elements)?;
                    definition.description = owned_description;
                    OwnedFixture::Inline(definition)
                }
                crate::dto::GuiOwnedFixture::Reference { object } => {
                    let owner = session.address_of(resolved.object).document_id().clone();
                    OwnedFixture::Reference(reference_definition(session, &owner, object)?)
                }
            },
            transform: checked_transform(transform)?,
        },
        GuiLayoutFixtureKind::Group { members } => LayoutFixtureKind::Group {
            members: members
                .into_iter()
                .map(|member| match member {
                    crate::dto::GuiGroupMember::Item { id } => {
                        donder_model::GroupMember::Item(FixtureInstanceId(id))
                    }
                    crate::dto::GuiGroupMember::Slice {
                        fixture,
                        start,
                        count,
                    } => donder_model::GroupMember::Slice {
                        fixture: FixtureInstanceId(fixture),
                        pixels: donder_model::PixelSpan { start, count },
                    },
                })
                .collect(),
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
    resolved: &ResolvedGuiObject,
    name: String,
    definition: OwnedFixture,
    parent: Option<u32>,
    transform: crate::dto::Transform,
) -> Result<(), GuiMutationError> {
    let name = super::model::typed_name(&name)?;
    let transform = checked_transform(transform)?;
    let fixtures = resolved.view::<Layout>(session).object().iter_fixtures();
    let id = FixtureInstanceId(next_id(fixtures.map(|fixture| fixture.id.0))?);
    update_layout(session, resolved, |layout| {
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
    delta: &crate::dto::Point3Meters,
) -> Result<(), GuiMutationError> {
    if let LayoutFixtureKind::Fixture { transform, .. } = &mut fixture.kind {
        transform.position = super::fixture::checked_point(crate::dto::Point3Meters {
            x_meters: transform.position.x.as_meters_f32() + delta.x_meters,
            y_meters: transform.position.y.as_meters_f32() + delta.y_meters,
            z_meters: transform.position.z.as_meters_f32() + delta.z_meters,
        })?;
    }
    Ok(())
}
