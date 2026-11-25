use super::fixture::{checked_transform, reference_definition};
use super::{GuiMutationError, ResolvedGuiObject, next_id};
use crate::dto::{GuiLayoutFixture, GuiLayoutFixtureKind, LayoutGuiEdit};
use donder_model::{FixtureDefinition, FixtureDefinitionId};
use donder_model::{FixtureId, Layout, LayoutFixture, LayoutFixtureKind};
use donder_project_io::ProjectSession;

pub(super) fn edit_layout(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: LayoutGuiEdit,
) -> Result<(), GuiMutationError> {
    match edit {
        LayoutGuiEdit::MoveMember {
            id: member,
            from,
            to,
            before,
        } => update_layout(session, resolved, |layout| {
            layout.move_member(
                FixtureId(member),
                from.map(FixtureId),
                to.map(FixtureId),
                before.map(FixtureId),
            )
        })?,
        LayoutGuiEdit::AddMember {
            id: member,
            to,
            before,
        } => update_layout(session, resolved, |layout| {
            layout.add_member(FixtureId(member), to.map(FixtureId), before.map(FixtureId))
        })?,
        LayoutGuiEdit::RemoveMember { id: member, from } => {
            update_layout(session, resolved, |layout| {
                layout.remove_member(FixtureId(member), from.map(FixtureId))
            })?
        }
        LayoutGuiEdit::RemoveItems { ids } => update_layout(session, resolved, |layout| {
            layout.remove_items(&ids.into_iter().map(FixtureId).collect::<Vec<_>>())
        })?,
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
                let fixture = FixtureId(*fixture_id);
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
                    let copy = duplicate(session, resolved, FixtureId(*fixture_id))?;
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
                    session
                        .put((resolved.id(), layout))
                        .map_err(GuiMutationError::from)?;
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
            add_instance(session, resolved, name, definition, parent, transform)?;
        }
        LayoutGuiEdit::SetFixtures { fixtures, root } => {
            let fixtures = fixtures
                .into_iter()
                .map(|fixture| domain_fixture(session, fixture))
                .collect::<Result<_, _>>()?;
            update_layout(session, resolved, |layout| {
                layout.fixtures = fixtures;
                layout.root = root.into_iter().map(FixtureId).collect();
                Ok(())
            })?;
        }
    }
    Ok(())
}

/// Duplicate `fixture` of the open layout; each definition it copies is
/// declared beside the layout, named after its copied fixture.
fn duplicate(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    fixture: FixtureId,
) -> Result<FixtureId, GuiMutationError> {
    let document = resolved.document_id().clone();
    session.edit(|edit| {
        let (copy, definitions) =
            donder_model::duplicate_layout_fixture(edit.project_mut(), fixture)?;
        for (fixture, definition) in definitions {
            let Some(placed) = edit.session().project().layout().object().fixture(fixture) else {
                unreachable!("a copy is in its layout");
            };
            let name = placed.name.as_str().to_string();
            edit.add_object(&document, definition.into(), &name)?;
        }
        Ok(copy)
    })
}

/// The definitions `layout` places.
fn placed_definitions(layout: &Layout) -> std::collections::HashSet<FixtureDefinitionId> {
    layout
        .fixtures
        .iter()
        .filter_map(|fixture| match fixture.kind {
            LayoutFixtureKind::Fixture { definition, .. } => Some(definition),
            LayoutFixtureKind::Group { .. } => None,
        })
        .collect()
}

/// Edit the open layout. A definition declared in the layout's document that
/// the layout stops placing is deleted with the last fixture placing it.
fn update_layout(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    update: impl FnOnce(&mut Layout) -> Result<(), donder_model::EditRejection>,
) -> Result<(), GuiMutationError> {
    let before = Layout::clone(&resolved.view(session));
    let mut layout = before.clone();
    update(&mut layout)?;
    let placed = placed_definitions(&layout);
    let forgotten = placed_definitions(&before)
        .into_iter()
        .filter(|definition| {
            !placed.contains(definition)
                && session
                    .declaration((*definition).into())
                    .is_some_and(|(document, _)| document == resolved.document_id())
        })
        .collect::<Vec<_>>();
    session.edit(|edit| {
        edit.project_mut().put((resolved.id(), layout))?;
        for definition in forgotten {
            edit.remove_object(definition.into())?;
        }
        Ok::<_, GuiMutationError>(())
    })
}

fn domain_fixture(
    session: &ProjectSession,
    fixture: GuiLayoutFixture,
) -> Result<LayoutFixture, GuiMutationError> {
    let kind = match fixture.kind {
        GuiLayoutFixtureKind::Fixture {
            definition,
            transform,
        } => LayoutFixtureKind::Fixture {
            definition: reference_definition(session, definition)?,
            transform: checked_transform(transform)?,
        },
        GuiLayoutFixtureKind::Group { members } => LayoutFixtureKind::Group {
            members: members
                .into_iter()
                .map(|member| match member {
                    crate::dto::GuiGroupMember::Item { id } => {
                        donder_model::GroupMember::Item(FixtureId(id))
                    }
                    crate::dto::GuiGroupMember::Slice {
                        fixture,
                        start,
                        count,
                    } => donder_model::GroupMember::Slice {
                        fixture: FixtureId(fixture),
                        pixels: donder_model::PixelSpan { start, count },
                    },
                })
                .collect(),
        },
    };
    Ok(LayoutFixture {
        id: FixtureId(fixture.id),
        name: super::model::typed_name(&fixture.name)?,
        description: super::description::normalized(fixture.description),
        kind,
    })
}

fn add_instance(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    name: String,
    definition: FixtureDefinitionId,
    parent: Option<u32>,
    transform: crate::dto::Transform,
) -> Result<(), GuiMutationError> {
    let name = super::model::typed_name(&name)?;
    let transform = checked_transform(transform)?;
    let fixtures = resolved.view::<Layout>(session).object().iter_fixtures();
    let id = FixtureId(next_id(fixtures.map(|fixture| fixture.id.0))?);
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
        layout.add_member(id, parent.map(FixtureId), None)
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
