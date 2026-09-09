//! Typed element-tree editing. The caller owns transactionality.
use super::{ElementNode, ElementNodeId, ElementNodeKind, ElementTreeId};
use crate::{model::DawnProject, patch::PatchNode};

pub fn add_group(
    project: &mut DawnProject,
    tree_id: &ElementTreeId,
    name: String,
    parent: Option<ElementNodeId>,
) -> Result<(), String> {
    add_element(
        project,
        tree_id,
        ElementNode {
            name,
            kind: ElementNodeKind::Group {
                children: Vec::new(),
            },
        },
        parent,
    )?;
    Ok(())
}

pub fn add_element(
    project: &mut DawnProject,
    tree_id: &ElementTreeId,
    element: ElementNode,
    parent: Option<ElementNodeId>,
) -> Result<ElementNodeId, String> {
    if element.name.trim().is_empty() {
        return Err("Give the element a name.".into());
    }
    let tree = project
        .element_trees
        .get_mut(tree_id)
        .ok_or("Element tree was not found.")?;
    let id = ElementNodeId(
        tree.nodes
            .keys()
            .map(|id| id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("No element identifiers remain.")?,
    );
    tree.nodes.insert(id, element);
    tree.roots.push(id);
    move_element(project, tree_id, id, parent)?;
    Ok(id)
}

pub fn move_element(
    project: &mut DawnProject,
    tree_id: &ElementTreeId,
    id: ElementNodeId,
    parent: Option<ElementNodeId>,
) -> Result<(), String> {
    let tree = project
        .element_trees
        .get_mut(tree_id)
        .ok_or("Element tree was not found.")?;
    if !tree.nodes.contains_key(&id) {
        return Err("Element was not found.".into());
    }
    if parent == Some(id) {
        return Err("An element cannot be its own parent.".into());
    }
    tree.roots.retain(|candidate| *candidate != id);
    for node in tree.nodes.values_mut() {
        if let ElementNodeKind::Group { children } = &mut node.kind {
            children.retain(|candidate| *candidate != id);
        }
    }
    match parent {
        None => tree.roots.push(id),
        Some(parent) => match &mut tree
            .nodes
            .get_mut(&parent)
            .ok_or("Parent group was not found.")?
            .kind
        {
            ElementNodeKind::Group { children } => children.push(id),
            _ => return Err("The parent must be a group.".into()),
        },
    }
    tree.validate()
        .map_err(|error| format!("This move would create an invalid group hierarchy: {error:?}"))
}

/// Delete one unreferenced leaf (or empty group), along with its exclusively
/// bound preview instances and output destinations. Effect/control references
/// must be reassigned explicitly before deletion.
pub fn remove_element(
    project: &mut DawnProject,
    tree_id: &ElementTreeId,
    id: ElementNodeId,
) -> Result<(), String> {
    let tree = project
        .element_trees
        .get(tree_id)
        .ok_or("Element tree was not found.")?;
    let node = tree.nodes.get(&id).ok_or("Element was not found.")?;
    if matches!(&node.kind, ElementNodeKind::Group { children } if !children.is_empty()) {
        return Err("Move or delete this group's children first.".into());
    }
    for sequence in project.sequences.values() {
        if sequence
            .effects
            .iter()
            .any(|effect| effect.target.tree == *tree_id && effect.target.node == id)
            || sequence.control_clips.iter().any(|clip| {
                clip.target.selection().tree == *tree_id && clip.target.selection().node == id
            })
        {
            return Err(format!(
                "{} is used by sequence {}. Reassign or delete its clips first.",
                node.name,
                sequence.id.0.object()
            ));
        }
    }
    for preview in project
        .preview_layouts
        .values()
        .filter(|preview| preview.element_tree == *tree_id)
    {
        for prop in &preview.props {
            if prop.bindings.iter().any(|binding| binding.node == id)
                && prop.bindings.iter().any(|binding| binding.node != id)
            {
                return Err(format!(
                    "{} is also used by preview prop {}. Change that prop's bindings first.",
                    node.name, prop.name
                ));
            }
        }
    }
    let mut destinations = Vec::new();
    for patch in project.patches.values() {
        let mut sinks = Vec::new();
        for (source_id, patch_node) in &patch.nodes {
            let PatchNode::Source(source) = patch_node else {
                continue;
            };
            if source.selection.tree != *tree_id {
                continue;
            }
            let contains = tree
                .flatten_selection(&source.selection)
                .map_err(|error| format!("Invalid patch selection: {error:?}"))?
                .iter()
                .any(|cell| cell.node == id);
            if !contains && source.selection.node != id {
                continue;
            }
            if source.selection.node != id {
                return Err(format!(
                    "{} participates in a group route. Remove that assignment first.",
                    node.name
                ));
            }
            let mut pending = vec![*source_id];
            let mut visited = std::collections::HashSet::new();
            while let Some(next) = pending.pop() {
                if !visited.insert(next) {
                    continue;
                }
                if matches!(patch.nodes.get(&next), Some(PatchNode::Sink(_))) {
                    sinks.push(next);
                }
                pending.extend(
                    patch
                        .edges
                        .iter()
                        .filter(|edge| edge.from == next)
                        .map(|edge| edge.to),
                );
            }
        }
        destinations.push((patch.id.clone(), sinks));
    }
    for (patch_id, mut sinks) in destinations {
        let patch = project
            .patches
            .get_mut(&patch_id)
            .ok_or("Patch was not found.")?;
        sinks.sort_unstable();
        sinks.dedup();
        for sink in sinks {
            patch.remove_output(sink)?;
        }
        // Disconnected source nodes have no destinations to prune.
        let sources = patch
            .nodes
            .iter()
            .filter_map(|(patch_id, node)| match node {
                PatchNode::Source(source)
                    if source.selection.tree == *tree_id && source.selection.node == id =>
                {
                    Some(*patch_id)
                }
                _ => None,
            })
            .collect::<std::collections::HashSet<_>>();
        if patch.edges.iter().any(|edge| sources.contains(&edge.from)) {
            return Err("Remove the light's unfinished patch branch before deleting it.".into());
        }
        patch
            .nodes
            .retain(|patch_id, _| !sources.contains(patch_id));
    }
    for layout in project
        .preview_layouts
        .values_mut()
        .filter(|layout| layout.element_tree == *tree_id)
    {
        layout
            .props
            .retain(|prop| !prop.bindings.iter().any(|binding| binding.node == id));
    }
    let tree = project
        .element_trees
        .get_mut(tree_id)
        .ok_or("Element tree was not found.")?;
    tree.roots.retain(|candidate| *candidate != id);
    for node in tree.nodes.values_mut() {
        if let ElementNodeKind::Group { children } = &mut node.kind {
            children.retain(|candidate| *candidate != id);
        }
    }
    tree.nodes.shift_remove(&id);
    Ok(())
}
