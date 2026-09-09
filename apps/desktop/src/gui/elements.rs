use super::setup::ensure_owned_target;
use super::{GuiMutationError, ResolvedGuiObject, blocked, patch};
use crate::dto::{
    ElementTreeGuiDocument, ElementTreeGuiEdit, GuiDocument, SetupElementKind, SetupElementNode,
};
use dawn_language::element::{ElementNodeId, ElementNodeKind, ElementTree, ElementTreeId};
use dawn_project_io::ProjectSession;
use std::collections::HashMap;
mod controls;

pub(super) fn project_document(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    match project_tree(session, &ElementTreeId(resolved.identity.clone())) {
        Ok(document) => GuiDocument::ElementTree { document },
        Err(error) => blocked(error.message(), Vec::new()),
    }
}

pub(super) fn project_tree(
    session: &ProjectSession,
    id: &ElementTreeId,
) -> Result<ElementTreeGuiDocument, GuiMutationError> {
    let tree = session
        .project
        .element_trees
        .get(id)
        .ok_or_else(|| GuiMutationError::Invalid("Element tree was not found.".into()))?;
    Ok(ElementTreeGuiDocument {
        path: crate::source_documents::editor_path(session, id.0.document_id())
            .ok_or_else(|| {
                GuiMutationError::Invalid("Element source has no file location.".into())
            })?
            .to_string(),
        object_key: id.0.object().to_string(),
        source_ref: patch::object_ref(&id.0, dawn_project_io::SourceObjectKind::ElementTree),
        read_only: !session.source.is_project_owned(id.0.document_id()),
        root_ids: tree.roots.iter().map(|id| id.0).collect(),
        elements: project_nodes(tree),
        profiles: patch::profiles(session),
    })
}

pub(super) fn project_nodes(tree: &ElementTree) -> Vec<SetupElementNode> {
    let parents = tree
        .nodes
        .iter()
        .flat_map(|(parent, node)| match &node.kind {
            ElementNodeKind::Group { children } => children
                .iter()
                .map(|child| (*child, *parent))
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect::<HashMap<_, _>>();
    tree.nodes
        .iter()
        .map(|(id, node)| {
            let (kind, children, capability, profile) = match &node.kind {
                ElementNodeKind::Group { children } => (
                    SetupElementKind::Group,
                    children.iter().map(|id| id.0).collect(),
                    None,
                    None,
                ),
                ElementNodeKind::Color { capability, .. } => (
                    SetupElementKind::Color,
                    Vec::new(),
                    Some(patch::project_capability(capability)),
                    None,
                ),
                ElementNodeKind::Scalar { .. } => {
                    (SetupElementKind::Scalar, Vec::new(), None, None)
                }
                ElementNodeKind::Indexed { .. } => {
                    (SetupElementKind::Indexed, Vec::new(), None, None)
                }
                ElementNodeKind::Fixture { profile } => (
                    SetupElementKind::Fixture,
                    Vec::new(),
                    None,
                    Some(super::setup::source_key(&profile.0)),
                ),
            };
            SetupElementNode {
                id: id.0,
                name: node.name.clone(),
                kind,
                parent: parents.get(id).map(|id| id.0),
                children,
                cell_count: node.kind.cell_count(),
                control_definition: controls::project_kind(&node.kind),
                color_component_count: match &node.kind {
                    ElementNodeKind::Color { capability, .. } => {
                        Some(dawn_language::patch::color_component_count(capability) as u32)
                    }
                    _ => None,
                },
                capability,
                profile,
            }
        })
        .collect()
}

pub(super) fn edit(
    session: &mut ProjectSession,
    tree_id: &ElementTreeId,
    edit: ElementTreeGuiEdit,
) -> Result<(), GuiMutationError> {
    ensure_owned_target(session, &tree_id.0)?;
    match edit {
        ElementTreeGuiEdit::AddControlElement {
            name,
            parent,
            definition,
        } => controls::edit(session, tree_id, None, name, parent, definition)?,
        ElementTreeGuiEdit::UpdateControlElement {
            id,
            name,
            definition,
        } => controls::edit(session, tree_id, Some(id), name, None, definition)?,
        ElementTreeGuiEdit::UpdateColorCapability {
            id,
            capability,
            component_order,
        } => {
            ensure_owned_target(session, &tree_id.0)?;
            ensure_patch_users_owned(session, tree_id, ElementNodeId(id))?;
            dawn_language::setup::authoring::update_color_capability(
                &mut session.project,
                tree_id,
                ElementNodeId(id),
                patch::domain_capability(capability)?,
                component_order,
            )
            .map_err(GuiMutationError::Invalid)?;
        }
        ElementTreeGuiEdit::AddGroup { name, parent } => {
            ensure_owned_target(session, &tree_id.0)?;
            dawn_language::element::authoring::add_group(
                &mut session.project,
                tree_id,
                name,
                parent.map(ElementNodeId),
            )
            .map_err(GuiMutationError::Invalid)?;
        }
        ElementTreeGuiEdit::MoveElement { id, parent } => {
            ensure_owned_target(session, &tree_id.0)?;
            dawn_language::element::authoring::move_element(
                &mut session.project,
                tree_id,
                ElementNodeId(id),
                parent.map(ElementNodeId),
            )
            .map_err(GuiMutationError::Invalid)?;
        }
        ElementTreeGuiEdit::DeleteElement { id } => {
            ensure_owned_target(session, &tree_id.0)?;
            ensure_layout_users_owned(session, tree_id, ElementNodeId(id))?;
            ensure_patch_users_owned(session, tree_id, ElementNodeId(id))?;
            dawn_language::element::authoring::remove_element(
                &mut session.project,
                tree_id,
                ElementNodeId(id),
            )
            .map_err(GuiMutationError::Invalid)?;
        }
        ElementTreeGuiEdit::RenameElement { id, name } => {
            if name.trim().is_empty() {
                return Err(GuiMutationError::Invalid(
                    "Element name cannot be empty.".to_string(),
                ));
            }
            tree_mut(session, tree_id)?
                .nodes
                .get_mut(&ElementNodeId(id))
                .ok_or_else(|| GuiMutationError::Invalid("Element was not found.".to_string()))?
                .name = name;
        }
        ElementTreeGuiEdit::ReorderElements {
            parent,
            ordered_ids,
        } => {
            let ordered = ordered_ids
                .into_iter()
                .map(ElementNodeId)
                .collect::<Vec<_>>();
            let tree = tree_mut(session, tree_id)?;
            if ordered.iter().any(|id| !tree.nodes.contains_key(id)) {
                return Err(GuiMutationError::Invalid(
                    "Element order references a missing node.".to_string(),
                ));
            }
            if let Some(parent) = parent {
                let node = tree.nodes.get_mut(&ElementNodeId(parent)).ok_or_else(|| {
                    GuiMutationError::Invalid("Parent element was not found.".to_string())
                })?;
                let ElementNodeKind::Group { children } = &mut node.kind else {
                    return Err(GuiMutationError::Invalid(
                        "Parent element is not a group.".to_string(),
                    ));
                };
                if children.len() != ordered.len()
                    || !children.iter().all(|child| ordered.contains(child))
                {
                    return Err(GuiMutationError::Invalid(
                        "Reorder must contain exactly the group's current children.".to_string(),
                    ));
                }
                *children = ordered;
            } else {
                if tree.roots.len() != ordered.len()
                    || !tree.roots.iter().all(|root| ordered.contains(root))
                {
                    return Err(GuiMutationError::Invalid(
                        "Reorder must contain exactly the current roots.".to_string(),
                    ));
                }
                tree.roots = ordered;
            }
        }
    }
    session.project.element_trees[tree_id]
        .validate()
        .map_err(|error| GuiMutationError::Invalid(format!("Invalid element tree: {error:?}")))?;
    Ok(())
}

pub(super) fn tree_mut<'a>(
    session: &'a mut ProjectSession,
    id: &dawn_language::element::ElementTreeId,
) -> Result<&'a mut dawn_language::element::ElementTree, GuiMutationError> {
    ensure_owned_target(session, &id.0)?;
    session
        .project
        .element_trees
        .get_mut(id)
        .ok_or_else(|| GuiMutationError::Invalid("Element tree is missing.".to_string()))
}

pub(super) fn ensure_patch_users_owned(
    session: &ProjectSession,
    tree: &ElementTreeId,
    node: ElementNodeId,
) -> Result<(), GuiMutationError> {
    for patch in session.project.patches.values() {
        if patch.nodes.values().any(|candidate| matches!(candidate, dawn_language::patch::PatchNode::Source(source) if source.selection.tree == *tree && source.selection.node == node)) {
            ensure_owned_target(session, &patch.id.0)?;
        }
    }
    Ok(())
}

fn ensure_layout_users_owned(
    session: &ProjectSession,
    tree: &ElementTreeId,
    node: ElementNodeId,
) -> Result<(), GuiMutationError> {
    for layout in session
        .project
        .preview_layouts
        .values()
        .filter(|layout| layout.element_tree == *tree)
    {
        if layout
            .props
            .iter()
            .any(|prop| prop.bindings.iter().any(|binding| binding.node == node))
        {
            ensure_owned_target(session, &layout.id.0)?;
        }
    }
    Ok(())
}
