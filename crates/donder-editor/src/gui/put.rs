//! The generic edits: write a value at a site, or remove what a site names.
//! A site's member is copied, its field set, and the member written back, so
//! every editor shares one way to set a name, a color, a clip's timing or a
//! parameter. What else names a removed member gives way to its removal.
use donder_model::{
    Always, ClipId, CompositionGraphNodeKind, Field, LayerId, Mark, Member, MemberRef, MemberValue,
    ObjectId, ObjectKind, Placement, ProjectEdit, Site,
};
use donder_project_io::ProjectSession;

use super::description::normalized;
use super::model::{
    chosen_effect, clip_scope, curve_from_points, identifier, param_value_from_gui, parse_color,
    target_from_gui, typed_name,
};
use super::{GuiMutationError, ResolvedGuiObject, checked_gui_duration, checked_gui_span};
use crate::dto::{GuiDocumentRequest, GuiField, GuiMember, GuiSite, GuiValue};

fn invalid(message: &str) -> GuiMutationError {
    GuiMutationError::Invalid(message.into())
}

/// The object a site names: the one its object names, or the open one.
fn target(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
    site: &GuiSite,
) -> Result<ResolvedGuiObject, GuiMutationError> {
    match &site.object {
        None => Ok(resolved.clone()),
        Some(object) => super::resolve_request(
            session,
            &GuiDocumentRequest {
                project_revision: 0,
                path: object.path.clone(),
                object_key: object.object_key.clone(),
            },
        )
        .map_err(GuiMutationError::Invalid),
    }
}

/// The model's site for `site` of `object`.
fn model_site(object: &ResolvedGuiObject, site: &GuiSite) -> Result<Site, GuiMutationError> {
    let member = site.member.as_ref().map(|member| match *member {
        GuiMember::Clip { id } => Member::Clip(ClipId(id)),
        GuiMember::Layer { id } => Member::Layer(LayerId(id)),
        GuiMember::Marks { id } => Member::Marks(donder_model::MarkCollectionId(id)),
        GuiMember::Node { id } => Member::Node(donder_model::CompositionGraphNodeId(id)),
        GuiMember::Event { id } => Member::Event(donder_model::OnId(id)),
        GuiMember::Control { id } => Member::Control(donder_model::InputControlId(id)),
        GuiMember::Port { id } => Member::Port(donder_model::ControllerPortId(id)),
        GuiMember::Route { id } => Member::Route(donder_model::PixelRouteId(id)),
        GuiMember::LayoutItem { id } => Member::LayoutItem(donder_model::FixtureId(id)),
        GuiMember::Shape { id } => Member::Shape(donder_model::FixtureElementId(id)),
    });
    // Only a field that names a part of a member is needed to find it.
    let field = match &site.field {
        Some(GuiField::Binding { target }) => Some(Field::Binding(
            super::edit::automation_target_from_gui(target.clone())?,
        )),
        Some(GuiField::Played { name }) => Some(Field::Played(identifier(name)?)),
        Some(GuiField::Set { name }) => Some(Field::Set(identifier(name)?)),
        _ => None,
    };
    Ok(Site {
        object: object.object,
        member,
        field,
    })
}

/// Write `value` at each of `sites`; the names a rename stores.
pub(super) fn put(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    sites: Vec<GuiSite>,
    value: GuiValue,
) -> Result<Vec<String>, GuiMutationError> {
    let [first, ..] = sites.as_slice() else {
        return Err(invalid("Choose what to edit."));
    };
    if first.member.is_none() {
        let [site] = sites.as_slice() else {
            return Err(invalid("Edit one object at a time."));
        };
        let object = target(session, resolved, site)?;
        return put_object(session, &object, site.field.as_ref(), value);
    }
    let mut created = Vec::new();
    let mut edits = Vec::new();
    for site in &sites {
        let object = target(session, resolved, site)?;
        if site.field == Some(GuiField::Always) {
            return put_always(session, &object, site, value);
        }
        let model = model_site(&object, site)?;
        let mut member = session
            .project()
            .member(&model)
            .and_then(MemberRef::to_value)
            .ok_or_else(|| GuiMutationError::from(donder_model::EditRejection::MissingMember))?;
        let field = site.field.as_ref();
        created.extend(set(session, &object, &mut member, field, value.clone())?);
        edits.push(ProjectEdit::PutMember(
            Site {
                field: None,
                ..model
            },
            member,
        ));
    }
    session.apply_edits(edits).map_err(GuiMutationError::from)?;
    Ok(created)
}

/// Set `field` of `member` to `value`; the name a rename stores.
fn set(
    session: &mut ProjectSession,
    object: &ResolvedGuiObject,
    member: &mut MemberValue,
    field: Option<&GuiField>,
    value: GuiValue,
) -> Result<Option<String>, GuiMutationError> {
    use GuiField as F;
    use GuiValue as V;
    use MemberValue as M;
    match (member, field, value) {
        (member, Some(F::Name), V::Text { text }) => {
            let name = typed_name(&text)?;
            let slot = match member {
                M::Clip(clip) => &mut clip.name,
                M::Marks(marks) => &mut marks.name,
                M::Layer(layer) => &mut layer.name,
                M::Node(node) => match &mut node.kind {
                    CompositionGraphNodeKind::Operator(operator) => &mut operator.name,
                    _ => return Err(invalid("Layer and root nodes are named by what they show.")),
                },
                _ => return Err(invalid("This has no name to edit.")),
            };
            *slot = name.clone();
            return Ok(Some(name.as_str().to_string()));
        }
        (member, Some(F::Description), V::OptionalText { text }) => {
            let slot = match member {
                M::Clip(clip) => &mut clip.description,
                M::Marks(marks) => &mut marks.description,
                M::Layer(layer) => &mut layer.description,
                _ => return Err(invalid("This has no description to edit.")),
            };
            *slot = normalized(text);
        }
        (M::Marks(marks), Some(F::Color), V::Text { text }) => {
            marks.display_color = parse_color(&text)?;
        }
        (M::Layer(layer), Some(F::Color), V::Text { text }) => layer.color = parse_color(&text)?,
        (M::Clip(clip), Some(F::Layer), V::Id { id }) => effect(clip)?.layer = LayerId(id),
        (M::Clip(clip), Some(F::Scope), V::Scope { scope }) => {
            effect(clip)?.scope = clip_scope(scope);
        }
        (M::Clip(clip), Some(F::Start), V::Seconds { seconds }) => match &mut clip.placement {
            Placement::Timed { start, .. } => *start = super::checked_gui_time(seconds)?,
            Placement::Played(_) => return Err(invalid("A played clip has no start.")),
        },
        (M::Clip(clip), Some(F::Duration), V::Seconds { seconds }) => match &mut clip.placement {
            Placement::Timed { duration, .. } => *duration = checked_gui_duration(seconds)?,
            Placement::Played(_) => {
                return Err(invalid("A played clip plays for its action's length."));
            }
        },
        (M::Clip(clip), Some(F::Target), V::Target { target }) => {
            clip.target = target_from_gui(target);
        }
        (
            M::Clip(clip),
            Some(F::Definition),
            V::Effect {
                initial_color,
                effect,
            },
        ) => {
            let chosen = chosen_effect(session, effect, &initial_color)?;
            let effect = self::effect(clip)?;
            (effect.definition, _, effect.params) = chosen;
            // Played values name the old effect's parameters.
            if let Placement::Played(values) = &mut clip.placement {
                values.clear();
            }
        }
        (member, Some(F::Param { name }), V::Param { value }) => {
            let value = param_value_from_gui(session, object.document_id(), value)?;
            let params = match member {
                M::Clip(clip) => &mut effect(clip)?.params,
                M::Node(node) => match &mut node.kind {
                    CompositionGraphNodeKind::Operator(operator) => &mut operator.params,
                    _ => return Err(invalid("Graph node is not an operator.")),
                },
                _ => return Err(invalid("This has no parameters.")),
            };
            params.insert(identifier(name)?, value);
        }
        (M::Marks(collection), Some(F::Points), V::Marks { marks }) => {
            let mut marks = marks
                .into_iter()
                .map(|mark| {
                    Ok(Mark {
                        time: super::checked_gui_time(mark.time_seconds)?,
                        duration: checked_gui_span(mark.duration_seconds)?,
                        label: mark
                            .label
                            .map(|label| label.trim().to_string())
                            .filter(|label| !label.is_empty()),
                    })
                })
                .collect::<Result<Vec<_>, GuiMutationError>>()?;
            marks.sort_by_key(|mark| mark.time.0);
            collection.marks = marks;
        }
        (M::Clip(clip), Some(F::Points), V::Curve { points }) => {
            let mut curve = curve_from_points(points);
            super::model::collapse_coincident_points(&mut curve);
            envelope(clip)?.curve = curve;
        }
        (M::Clip(clip), Some(F::Binding { target }), V::Window { window }) => {
            let target = super::edit::automation_target_from_gui(target.clone())?;
            envelope(clip)?
                .bindings
                .iter_mut()
                .find(|binding| binding.target == target)
                .ok_or_else(|| invalid("Automation binding was not found."))?
                .window = window.into();
        }
        (M::Event(on), None, V::Actions { actions }) => {
            on.actions = actions
                .into_iter()
                .map(|action| super::console::action_from_gui(session, action))
                .collect::<Result<_, _>>()?;
        }
        _ => return Err(invalid("That value does not fit that field.")),
    }
    Ok(None)
}

/// The effect a clip plays, for a field only effects have.
fn effect(clip: &mut donder_model::Clip) -> Result<&mut donder_model::Effect, GuiMutationError> {
    clip.effect_mut()
        .ok_or_else(|| invalid("An envelope plays no effect."))
}

/// The envelope a clip is, for a field only envelopes have.
fn envelope(
    clip: &mut donder_model::Clip,
) -> Result<&mut donder_model::Envelope, GuiMutationError> {
    clip.envelope_mut()
        .ok_or_else(|| invalid("Only an envelope has a curve and bindings."))
}

/// Write a field of an object as a whole.
fn put_object(
    session: &mut ProjectSession,
    object: &ResolvedGuiObject,
    field: Option<&GuiField>,
    value: GuiValue,
) -> Result<Vec<String>, GuiMutationError> {
    use GuiField as F;
    use GuiValue as V;
    match (object.kind, field, value) {
        (_, Some(F::Name), V::Text { text }) => {
            let name = typed_name(&text)?;
            session
                .rename_object(object.object, name.clone())
                .map_err(GuiMutationError::Invalid)?;
            return Ok(vec![name.as_str().to_string()]);
        }
        (_, Some(F::Description), V::OptionalText { text }) => {
            super::description::edit(session, object, text)?;
        }
        (ObjectKind::Sequence, Some(field), value) => {
            let mut sequence = donder_model::Sequence::clone(&object.view(session));
            match (field, value) {
                (F::Duration, V::Seconds { seconds }) => {
                    sequence.duration = checked_gui_duration(seconds)?;
                }
                (F::FrameRate, V::Count { count }) => sequence.frame_rate = count,
                (F::Audio, V::OptionalText { text }) => {
                    sequence.audio = match text {
                        Some(path) => donder_model::SequenceAudio::Asset(
                            session
                                .reference_asset(object.document_id(), &path)
                                .map_err(|error| {
                                    GuiMutationError::Invalid(format!("Audio file {path}: {error}"))
                                })?,
                        ),
                        None => donder_model::SequenceAudio::None,
                    };
                }
                _ => return Err(invalid("That value does not fit that field.")),
            }
            session.put((object.id(), sequence))?;
        }
        (ObjectKind::Console, Some(F::FrameRate), V::Count { count }) => {
            let mut console = donder_model::Console::clone(&object.view(session));
            console.frame_rate = count;
            session.put((object.id(), console))?;
        }
        (ObjectKind::Curve, Some(F::Points), V::Curve { points }) => {
            super::library::edit_curve(session, object, points)?;
        }
        (ObjectKind::Gradient, Some(F::Points), V::Gradient { gradient }) => {
            super::library::edit_gradient(session, object, gradient)?;
        }
        (ObjectKind::Controller, None, V::Controller { config, ports }) => {
            super::controller::edit(session, object, config, ports)?;
        }
        (ObjectKind::InputDevice, None, V::InputDevice { driver, controls }) => {
            super::input_device::edit(session, object, driver, controls)?;
        }
        (ObjectKind::Project, None, V::Routes { routes }) => {
            super::patch::replace(session, routes)?;
        }
        _ => return Err(invalid("That value does not fit that field.")),
    }
    Ok(Vec::new())
}

/// Play a console clip for the whole session, repeating every `seconds`, or
/// no longer.
fn put_always(
    session: &mut ProjectSession,
    object: &ResolvedGuiObject,
    site: &GuiSite,
    value: GuiValue,
) -> Result<Vec<String>, GuiMutationError> {
    let (Some(GuiMember::Clip { id }), ObjectKind::Console, GuiValue::OptionalSeconds { seconds }) =
        (&site.member, object.kind, value)
    else {
        return Err(invalid("That value does not fit that field."));
    };
    let clip = ClipId(*id);
    let mut console = donder_model::Console::clone(&object.view(session));
    match seconds {
        Some(seconds) => {
            let length = checked_gui_duration(seconds)?;
            match console.always.iter_mut().find(|always| always.clip == clip) {
                Some(always) => always.length = length,
                None => console.always.push(Always { clip, length }),
            }
        }
        None => console.always.retain(|always| always.clip != clip),
    }
    session.put((object.id(), console))?;
    Ok(Vec::new())
}

/// What a removal took: the object's name, and the document it alone was
/// declared in, which the caller deletes.
pub struct DeletedMember {
    pub name: String,
    pub document: Option<donder_project_io::DocumentId>,
}

/// Remove what `site` names: an object of the project, or a member or part
/// of one. What named it gives way.
pub(super) fn remove(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    site: &GuiSite,
) -> Result<Option<DeletedMember>, GuiMutationError> {
    let object = target(session, resolved, site)?;
    if site.member.is_none() {
        return remove_object(session, object.object).map(Some);
    }
    let model = model_site(&object, site)?;
    session
        .apply_edits([ProjectEdit::RemoveMember(model)])
        .map_err(GuiMutationError::from)?;
    Ok(None)
}

/// Remove the object `id` from the project, with the file it alone was
/// declared in.
fn remove_object(
    session: &mut ProjectSession,
    id: ObjectId,
) -> Result<DeletedMember, GuiMutationError> {
    let name = session.address_of(id).object().to_string();
    if matches!(id.kind(), ObjectKind::Layout | ObjectKind::Project) {
        return Err(GuiMutationError::Invalid(format!(
            "A project needs its {}.",
            id.kind()
        )));
    }
    if let Some(document) = super::ownership::own_document(session, id) {
        donder_project_io::remove_documents(session, std::slice::from_ref(&document))
            .map_err(GuiMutationError::from)?;
        return Ok(DeletedMember {
            name,
            document: Some(document),
        });
    }
    session.edit(|edit| Ok::<_, GuiMutationError>(edit.remove_object(id)?))?;
    Ok(DeletedMember {
        name,
        document: None,
    })
}
