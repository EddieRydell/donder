//! Definition edits preserve shared identity. Callers own transactionality.
use super::{PropDefinition, PropDefinitionId};
use crate::element::{ElementCellAddress, ElementNodeKind};
use crate::identity::SourceIdentity;
use crate::model::DawnProject;
use crate::setup::authoring::routing::{pixel_routes, resize_pixel_routes};
use indexmap::IndexSet;

/// Resize complete pixel-light placements and their guided outputs together.
/// Returns the source objects actually changed for the IO ownership boundary.
pub fn update_definition(
    project: &mut DawnProject,
    id: &PropDefinitionId,
    definition: PropDefinition,
) -> Result<Vec<SourceIdentity>, String> {
    let original = project
        .definitions
        .props
        .definitions
        .get(id)
        .ok_or("Fixture definition was not found.")?;
    let old_count = original.geometry.point_count();
    let count =
        u32::try_from(definition.geometry.point_count()).map_err(|_| "Too many fixture points.")?;
    if count == 0 {
        return Err("A fixture needs at least one point.".into());
    }
    let mut changed = IndexSet::from([id.0.clone()]);
    if count as usize != old_count {
        let mut elements = IndexSet::new();
        for layout in project.preview_layouts.values() {
            for placement in layout
                .props
                .iter()
                .filter(|placement| placement.definition == *id)
            {
                let node = placement
                    .bindings
                    .first()
                    .ok_or("The fixture placement has no element bindings.")?
                    .node;
                let complete = placement.bindings.len() == old_count
                    && placement
                        .bindings
                        .iter()
                        .enumerate()
                        .all(|(index, binding)| {
                            binding.node == node && binding.cell as usize == index
                        });
                let tree = project
                    .element_trees
                    .get(&layout.element_tree)
                    .ok_or("Element tree was not found.")?;
                if !complete
                    || !matches!(tree.nodes.get(&node).map(|node| &node.kind), Some(ElementNodeKind::Color { cells, .. }) if *cells as usize == old_count)
                {
                    return Err(format!(
                        "Placement {} uses custom bindings. Pixel-count changes require a complete color-element placement.",
                        placement.name
                    ));
                }
                elements.insert((layout.element_tree.clone(), node));
                changed.insert(layout.id.0.clone());
            }
        }
        // Another definition cannot silently inherit a different element size.
        for layout in project.preview_layouts.values() {
            for placement in &layout.props {
                if placement.definition != *id
                    && placement.bindings.iter().any(|binding| {
                        elements.contains(&(layout.element_tree.clone(), binding.node))
                    })
                {
                    return Err(format!(
                        "Placement {} shares this element with another fixture definition. Rebind it before changing the pixel count.",
                        placement.name
                    ));
                }
            }
        }
        let mut outputs = Vec::new();
        for (tree, node) in &elements {
            for patch in project.patches.keys() {
                let routes = pixel_routes(project, tree, patch, *node)?;
                if !routes.is_empty() {
                    changed.insert(patch.0.clone());
                    outputs.push((tree.clone(), patch.clone(), routes));
                }
            }
        }
        for (tree, node) in elements {
            let element = project
                .element_trees
                .get_mut(&tree)
                .and_then(|tree| tree.nodes.get_mut(&node))
                .ok_or("Element was not found.")?;
            let ElementNodeKind::Color { cells, .. } = &mut element.kind else {
                return Err("Choose a color element.".into());
            };
            *cells = count;
            changed.insert(tree.0);
        }
        for layout in project.preview_layouts.values_mut() {
            for placement in layout
                .props
                .iter_mut()
                .filter(|placement| placement.definition == *id)
            {
                let node = placement
                    .bindings
                    .first()
                    .ok_or("The fixture placement has no element bindings.")?
                    .node;
                placement.bindings = (0..count)
                    .map(|cell| ElementCellAddress { node, cell })
                    .collect();
            }
        }
        for (tree, patch, routes) in outputs {
            resize_pixel_routes(project, &tree, &patch, routes)?;
        }
    }
    project
        .definitions
        .props
        .definitions
        .insert(id.clone(), definition);
    Ok(changed.into_iter().collect())
}

pub struct FixturePlacement {
    pub name: String,
    pub capability: crate::element::ColorCapability,
    pub parent: Option<crate::element::ElementNodeId>,
    pub definition: PropDefinitionId,
    pub position: crate::values::Point3,
}

pub fn place_fixture(
    project: &mut DawnProject,
    layout_id: &super::PreviewLayoutId,
    placement: FixturePlacement,
) -> Result<super::PropInstanceId, String> {
    let definition = project
        .definitions
        .props
        .definitions
        .get(&placement.definition)
        .ok_or("Fixture definition was not found.")?;
    let cells =
        u32::try_from(definition.geometry.point_count()).map_err(|_| "Too many fixture points.")?;
    if cells == 0 {
        return Err("A fixture needs at least one point.".into());
    }
    placement
        .capability
        .validate()
        .map_err(|error| format!("Invalid color capability: {error:?}"))?;
    let tree_id = project
        .preview_layouts
        .get(layout_id)
        .ok_or("Layout was not found.")?
        .element_tree
        .clone();
    let node = crate::element::authoring::add_element(
        project,
        &tree_id,
        crate::element::ElementNode {
            name: placement.name.clone(),
            kind: ElementNodeKind::Color {
                cells,
                capability: placement.capability,
            },
        },
        placement.parent,
    )?;
    let layout = project
        .preview_layouts
        .get_mut(layout_id)
        .ok_or("Layout was not found.")?;
    let id = next_placement_id(layout)?;
    layout.props.push(super::PropInstance {
        id,
        name: placement.name,
        definition: placement.definition,
        position: placement.position,
        rotation: crate::values::Rotation3::default(),
        scale: crate::values::Scale3::default(),
        bindings: (0..cells)
            .map(|cell| ElementCellAddress { node, cell })
            .collect(),
    });
    Ok(id)
}

/// A new light gets new elements while retaining the shared fixture definition.
/// Existing output assignments remain attached to the original elements.
pub fn duplicate_placement(
    project: &mut DawnProject,
    layout_id: &super::PreviewLayoutId,
    id: super::PropInstanceId,
) -> Result<(), String> {
    let layout = project
        .preview_layouts
        .get(layout_id)
        .ok_or("Layout was not found.")?;
    let mut placement = layout
        .props
        .iter()
        .find(|placement| placement.id == id)
        .ok_or("Placement was not found.")?
        .clone();
    let tree_id = layout.element_tree.clone();
    let mut copies = indexmap::IndexMap::new();
    for binding in &placement.bindings {
        if copies.contains_key(&binding.node) {
            continue;
        }
        let tree = project
            .element_trees
            .get(&tree_id)
            .ok_or("Element tree was not found.")?;
        let mut element = tree
            .nodes
            .get(&binding.node)
            .ok_or("Element was not found.")?
            .clone();
        element.name.push_str(" copy");
        let parent = tree.nodes.iter().find_map(|(id, node)| match &node.kind {
            ElementNodeKind::Group { children } if children.contains(&binding.node) => Some(*id),
            _ => None,
        });
        let copy = crate::element::authoring::add_element(project, &tree_id, element, parent)?;
        copies.insert(binding.node, copy);
    }
    for binding in &mut placement.bindings {
        binding.node = copies[&binding.node];
    }
    let layout = project
        .preview_layouts
        .get_mut(layout_id)
        .ok_or("Layout was not found.")?;
    placement.id = next_placement_id(layout)?;
    placement.name.push_str(" copy");
    layout.props.push(placement);
    Ok(())
}

fn next_placement_id(layout: &super::PreviewLayout) -> Result<super::PropInstanceId, String> {
    layout
        .props
        .iter()
        .map(|prop| prop.id.0)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .map(super::PropInstanceId)
        .ok_or_else(|| "No placement identifiers remain.".into())
}
