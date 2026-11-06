//! The generic edits of an open object's parts: renaming, describing,
//! coloring and deleting the item a `GuiItem` names.
use donder_model::{Console, ConsoleClipId, MarkCollectionKey, ObjectKind, Sequence};
use donder_project_io::ProjectSession;

use super::console::{clip_mut, delete_clip, live_marks_mut};
use super::graph::{Composition, layer_mut, node_mut, replace_mark_collection};
use super::model::{mark_collection_mut, parse_color, typed_name};
use super::{GuiMutationError, ResolvedGuiObject};
use crate::dto::GuiItem;

pub(super) enum Verb {
    Rename(String),
    Describe(Option<String>),
    Color(String),
    Delete,
}

pub(super) fn edit(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    item: GuiItem,
    verb: Verb,
) -> Result<Vec<String>, GuiMutationError> {
    // A rename reports the name it stores, which the typed text becomes.
    let created = match &verb {
        Verb::Rename(name) => vec![typed_name(name)?.as_str().to_string()],
        _ => Vec::new(),
    };
    edit_item(session, resolved, item, verb)?;
    Ok(created)
}

fn edit_item(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    item: GuiItem,
    verb: Verb,
) -> Result<(), GuiMutationError> {
    match (resolved.kind, item, verb) {
        (_, GuiItem::Object, Verb::Describe(description)) => {
            super::description::edit(session, resolved, description)
        }
        (ObjectKind::Sequence, item, verb) => {
            let mut draft = session
                .project
                .get::<Sequence>(resolved.id())
                .cloned()
                .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".into()))?;
            sequence_item(&mut draft, item, verb)?;
            session.project.put(draft).map_err(GuiMutationError::from)
        }
        (ObjectKind::Console, item, verb) => {
            let mut draft = session
                .project
                .get::<Console>(resolved.id())
                .cloned()
                .ok_or_else(|| GuiMutationError::Invalid("Console was not found.".into()))?;
            console_item(&mut draft, item, verb)?;
            session.project.put(draft).map_err(GuiMutationError::from)
        }
        _ => Err(unsupported()),
    }
}

fn unsupported() -> GuiMutationError {
    GuiMutationError::Invalid("This edit does not apply to that item.".into())
}

fn sequence_item(
    sequence: &mut Sequence,
    item: GuiItem,
    verb: Verb,
) -> Result<(), GuiMutationError> {
    match (item, verb) {
        (GuiItem::Clip { id }, Verb::Rename(name)) => {
            super::model::clip_mut(sequence, id)?.name = typed_name(&name)?;
        }
        (GuiItem::Clip { id }, Verb::Describe(description)) => {
            super::model::clip_mut(sequence, id)?.description =
                super::description::normalized(description);
        }
        (GuiItem::MarkCollection { key }, Verb::Describe(description)) => {
            mark_collection_mut(sequence, &key)?.description =
                super::description::normalized(description);
        }
        (GuiItem::MarkCollection { key }, Verb::Color(color)) => {
            mark_collection_mut(sequence, &key)?.display_color = parse_color(&color)?;
        }
        (item, verb) => composition_item(sequence, item, verb)?,
    }
    Ok(())
}

fn console_item(console: &mut Console, item: GuiItem, verb: Verb) -> Result<(), GuiMutationError> {
    let description = super::description::normalized;
    match (item, verb) {
        (GuiItem::MarkCollection { key }, Verb::Describe(text)) => {
            live_marks_mut(console, &key)?.description = description(text);
        }
        (GuiItem::MarkCollection { key }, Verb::Color(color)) => {
            live_marks_mut(console, &key)?.display_color = parse_color(&color)?;
        }
        (GuiItem::Clip { id }, Verb::Rename(name)) => {
            clip_mut(console, ConsoleClipId(id))?.name = typed_name(&name)?;
        }
        (GuiItem::Clip { id }, Verb::Describe(text)) => {
            clip_mut(console, ConsoleClipId(id))?.description = description(text);
        }
        (GuiItem::Clip { id }, Verb::Delete) => {
            // The actions that play the clip and the bindings to it go with it.
            let id = clip_mut(console, ConsoleClipId(id))?.id;
            delete_clip(console, id);
        }
        (GuiItem::Event { id }, Verb::Delete) => {
            if !console.events.iter().any(|on| on.id.0 == id) {
                return Err(GuiMutationError::Invalid("Event was not found.".into()));
            }
            console.events.retain(|on| on.id.0 != id);
        }
        (item, verb) => composition_item(console, item, verb)?,
    }
    Ok(())
}

/// The layers, graph nodes and mark collections sequences and consoles share.
fn composition_item(
    composition: &mut impl Composition,
    item: GuiItem,
    verb: Verb,
) -> Result<(), GuiMutationError> {
    match (item, verb) {
        // Every parameter naming the collection follows a rename; after a
        // delete, those parameters name no marks.
        (GuiItem::MarkCollection { key }, Verb::Rename(name)) => {
            let to = MarkCollectionKey {
                name: typed_name(&name)?,
            };
            replace_mark_collection(composition, &key, Some(to))?;
        }
        (GuiItem::MarkCollection { key }, Verb::Delete) => {
            replace_mark_collection(composition, &key, None)?;
        }
        (GuiItem::Layer { id }, Verb::Rename(name)) => {
            layer_mut(composition, id)?.name = typed_name(&name)?;
        }
        (GuiItem::Layer { id }, Verb::Describe(description)) => {
            layer_mut(composition, id)?.description = super::description::normalized(description);
        }
        (GuiItem::Layer { id }, Verb::Color(color)) => {
            layer_mut(composition, id)?.color = parse_color(&color)?;
        }
        (GuiItem::GraphNode { node_id }, Verb::Rename(name)) => {
            let node_id = donder_model::CompositionGraphNodeId(node_id);
            let donder_model::CompositionGraphNodeKind::Operator(operator) =
                &mut node_mut(composition, &node_id)?.kind
            else {
                return Err(GuiMutationError::Invalid(
                    "Layer and root nodes are named by what they show.".to_string(),
                ));
            };
            operator.name = typed_name(&name)?;
        }
        _ => return Err(unsupported()),
    }
    Ok(())
}
