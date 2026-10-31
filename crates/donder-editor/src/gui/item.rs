//! The generic edits of an open object's parts: renaming, describing,
//! coloring and deleting the item a `GuiItem` names.
use donder_model::{Action, Console, ConsoleClipId, MarkCollectionKey, ObjectKind, Sequence};
use donder_project_io::ProjectSession;

use super::console::{clip_mut, console_mark_references_mut, delete_clip, live_marks_mut};
use super::graph::{Composition, layer_mut, node_mut};
use super::model::{mark_collection_mut, parse_color, parse_graph_node_id, typed_name};
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
        (GuiItem::MarkCollection { key }, Verb::Rename(name)) => {
            // A rename moves every clip parameter that names the collection.
            let name = typed_name(&name)?;
            let from = mark_collection_mut(sequence, &key)?.key.name.clone();
            mark_collection_mut(sequence, &key)?.key.name = name.clone();
            super::edit::mark_references_mut(sequence, |reference| {
                if let Some(collection) = reference
                    && collection.name == from
                {
                    collection.name = name.clone();
                }
            });
        }
        (GuiItem::MarkCollection { key }, Verb::Describe(description)) => {
            mark_collection_mut(sequence, &key)?.description =
                super::description::normalized(description);
        }
        (GuiItem::MarkCollection { key }, Verb::Color(color)) => {
            mark_collection_mut(sequence, &key)?.display_color = parse_color(&color)?;
        }
        (GuiItem::MarkCollection { key }, Verb::Delete) => {
            mark_collection_mut(sequence, &key)?;
            super::edit::mark_references_mut(sequence, |reference| {
                if reference
                    .as_ref()
                    .is_some_and(|collection| collection.name.as_str() == key)
                {
                    *reference = None;
                }
            });
            sequence
                .mark_collections
                .retain(|collection| collection.key.name.as_str() != key);
        }
        (item, verb) => composition_item(sequence, item, verb)?,
    }
    Ok(())
}

fn console_item(console: &mut Console, item: GuiItem, verb: Verb) -> Result<(), GuiMutationError> {
    let description = super::description::normalized;
    match (item, verb) {
        (GuiItem::MarkCollection { key }, Verb::Rename(name)) => {
            // Renaming live marks renames every parameter and action naming them.
            let to = MarkCollectionKey {
                name: typed_name(&name)?,
            };
            let from = live_marks_mut(console, &key)?.key.clone();
            live_marks_mut(console, &key)?.key = to.clone();
            console_mark_references_mut(console, |reference| {
                if reference.as_ref() == Some(&from) {
                    *reference = Some(to.clone());
                }
            });
            for action in console.events.iter_mut().flat_map(|on| &mut on.actions) {
                if let Action::Mark(key) = action
                    && *key == from
                {
                    *key = to.clone();
                }
            }
        }
        (GuiItem::MarkCollection { key }, Verb::Describe(text)) => {
            live_marks_mut(console, &key)?.description = description(text);
        }
        (GuiItem::MarkCollection { key }, Verb::Color(color)) => {
            live_marks_mut(console, &key)?.display_color = parse_color(&color)?;
        }
        (GuiItem::MarkCollection { key }, Verb::Delete) => {
            // Parameters keep working with no marks; actions adding marks go.
            let removed = live_marks_mut(console, &key)?.key.clone();
            console_mark_references_mut(console, |reference| {
                if reference.as_ref() == Some(&removed) {
                    *reference = None;
                }
            });
            for on in &mut console.events {
                on.actions
                    .retain(|action| !matches!(action, Action::Mark(key) if *key == removed));
            }
            console.live_marks.retain(|marks| marks.key != removed);
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

/// The layers and graph nodes sequences and consoles share.
fn composition_item(
    composition: &mut impl Composition,
    item: GuiItem,
    verb: Verb,
) -> Result<(), GuiMutationError> {
    match (item, verb) {
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
            let node_id = parse_graph_node_id(&node_id)?;
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
