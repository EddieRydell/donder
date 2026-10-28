//! The generic edits of an open object's parts: renaming, describing,
//! coloring and deleting the item a `GuiItem` names.
use donder_model::{MappingAction, MarkCollectionKey, ObjectKind, Rig, Sequence};
use donder_project_io::ProjectSession;

use super::graph::{Composition, layer_mut, node_mut};
use super::model::{mark_collection_mut, parse_color, parse_graph_node_id, typed_name};
use super::rig::{live_marks_mut, mapping_mut, rig_mark_references_mut, switch_mut, voice_mut};
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
        (ObjectKind::Rig, item, verb) => {
            let mut draft = session
                .project
                .get::<Rig>(resolved.id())
                .cloned()
                .ok_or_else(|| GuiMutationError::Invalid("Rig was not found.".into()))?;
            rig_item(&mut draft, item, verb)?;
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

fn rig_item(rig: &mut Rig, item: GuiItem, verb: Verb) -> Result<(), GuiMutationError> {
    let description = super::description::normalized;
    match (item, verb) {
        (GuiItem::MarkCollection { key }, Verb::Rename(name)) => {
            // Renaming live marks renames every parameter and mapping naming them.
            let to = MarkCollectionKey {
                name: typed_name(&name)?,
            };
            let from = live_marks_mut(rig, &key)?.key.clone();
            live_marks_mut(rig, &key)?.key = to.clone();
            rig_mark_references_mut(rig, |reference| {
                if reference.as_ref() == Some(&from) {
                    *reference = Some(to.clone());
                }
            });
            for mapping in &mut rig.mappings {
                if let MappingAction::Mark(key) = &mut mapping.action
                    && *key == from
                {
                    *key = to.clone();
                }
            }
        }
        (GuiItem::MarkCollection { key }, Verb::Describe(text)) => {
            live_marks_mut(rig, &key)?.description = description(text);
        }
        (GuiItem::MarkCollection { key }, Verb::Color(color)) => {
            live_marks_mut(rig, &key)?.display_color = parse_color(&color)?;
        }
        (GuiItem::MarkCollection { key }, Verb::Delete) => {
            let removed = live_marks_mut(rig, &key)?.key.clone();
            rig_mark_references_mut(rig, |reference| {
                if reference.as_ref() == Some(&removed) {
                    *reference = None;
                }
            });
            rig.live_marks.retain(|marks| marks.key != removed);
        }
        (GuiItem::Voice { id }, Verb::Rename(name)) => {
            voice_mut(rig, id)?.name = typed_name(&name)?
        }
        (GuiItem::Switch { id }, Verb::Rename(name)) => {
            switch_mut(rig, id)?.name = typed_name(&name)?;
        }
        (GuiItem::Mapping { id }, Verb::Rename(name)) => {
            mapping_mut(rig, id)?.name = typed_name(&name)?;
        }
        (GuiItem::Voice { id }, Verb::Describe(text)) => {
            voice_mut(rig, id)?.description = description(text);
        }
        (GuiItem::Switch { id }, Verb::Describe(text)) => {
            switch_mut(rig, id)?.description = description(text);
        }
        (GuiItem::Mapping { id }, Verb::Describe(text)) => {
            mapping_mut(rig, id)?.description = description(text);
        }
        (GuiItem::Voice { id }, Verb::Delete) => {
            voice_mut(rig, id)?;
            rig.voices.retain(|voice| voice.id.0 != id);
        }
        (GuiItem::Switch { id }, Verb::Delete) => {
            switch_mut(rig, id)?;
            rig.switches.retain(|switch| switch.id.0 != id);
        }
        (GuiItem::Mapping { id }, Verb::Delete) => {
            mapping_mut(rig, id)?;
            rig.mappings.retain(|mapping| mapping.id.0 != id);
        }
        (item, verb) => composition_item(rig, item, verb)?,
    }
    Ok(())
}

/// The layers and graph nodes sequences and rigs share.
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
