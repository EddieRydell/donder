pub mod authoring;

use std::collections::HashSet;

use indexmap::{IndexMap, IndexSet};

use crate::fixture_profile::FixtureProfileId;
use crate::identity::SourceIdentity;
use crate::values::Color;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ElementTreeId(pub SourceIdentity);

pub use dawn_runtime::element::ElementNodeId;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct IndexedOptionId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct EmitterId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct ElementTree {
    pub id: ElementTreeId,
    pub roots: Vec<ElementNodeId>,
    pub nodes: IndexMap<ElementNodeId, ElementNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ElementNode {
    pub name: String,
    pub kind: ElementNodeKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ElementNodeKind {
    Group {
        children: Vec<ElementNodeId>,
    },
    Color {
        cells: u32,
        capability: ColorCapability,
    },
    Scalar {
        cells: u32,
    },
    Indexed {
        cells: u32,
        options: Vec<IndexedOption>,
    },
    Fixture {
        profile: FixtureProfileId,
    },
}

impl ElementNodeKind {
    pub fn cell_count(&self) -> Option<u32> {
        match self {
            Self::Group { .. } => None,
            Self::Color { cells, .. } | Self::Scalar { cells } | Self::Indexed { cells, .. } => {
                Some(*cells)
            }
            Self::Fixture { .. } => Some(1),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ColorCapability {
    Rgb,
    Rgbw,
    Discrete {
        emitters: Vec<DiscreteEmitter>,
        mappings: Vec<DiscreteColorMapping>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiscreteEmitter {
    pub id: EmitterId,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiscreteColorMapping {
    pub color: Color,
    pub levels: IndexMap<EmitterId, f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IndexedOption {
    pub id: IndexedOptionId,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ElementSelection {
    pub tree: ElementTreeId,
    pub node: ElementNodeId,
    pub cells: Option<ElementCellRange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ElementCellRange {
    pub start: u32,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ElementCellAddress {
    pub node: ElementNodeId,
    pub cell: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ElementTreeValidationError {
    DuplicateRoot(ElementNodeId),
    MissingNode(ElementNodeId),
    MultipleParents(ElementNodeId),
    RootHasParent(ElementNodeId),
    UnreachableNode(ElementNodeId),
    Cycle(ElementNodeId),
    EmptyCells(ElementNodeId),
    DuplicateOption {
        node: ElementNodeId,
        option: IndexedOptionId,
    },
    EmptyOptions(ElementNodeId),
    DuplicateEmitter {
        node: ElementNodeId,
        emitter: EmitterId,
    },
    EmptyEmitters(ElementNodeId),
    EmptyMappings(ElementNodeId),
    DuplicateMappedColor {
        node: ElementNodeId,
        color: Color,
    },
    MissingMappedEmitter {
        node: ElementNodeId,
        emitter: EmitterId,
    },
    UnknownMappedEmitter {
        node: ElementNodeId,
        emitter: EmitterId,
    },
    InvalidEmitterLevel {
        node: ElementNodeId,
        emitter: EmitterId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ElementSelectionError {
    WrongTree,
    MissingNode(ElementNodeId),
    GroupCellRange(ElementNodeId),
    EmptyRange,
    RangeOutOfBounds {
        node: ElementNodeId,
        cell_count: u32,
    },
}

impl ElementTree {
    pub fn validate(&self) -> Result<(), ElementTreeValidationError> {
        let mut roots = IndexSet::new();
        for root in &self.roots {
            if !roots.insert(*root) {
                return Err(ElementTreeValidationError::DuplicateRoot(*root));
            }
            if !self.nodes.contains_key(root) {
                return Err(ElementTreeValidationError::MissingNode(*root));
            }
        }

        let mut parent = IndexMap::new();
        for (node_id, node) in &self.nodes {
            match &node.kind {
                ElementNodeKind::Group { children } => {
                    let mut local = IndexSet::new();
                    for child in children {
                        if !self.nodes.contains_key(child) {
                            return Err(ElementTreeValidationError::MissingNode(*child));
                        }
                        if !local.insert(*child) || parent.insert(*child, *node_id).is_some() {
                            return Err(ElementTreeValidationError::MultipleParents(*child));
                        }
                    }
                }
                ElementNodeKind::Color { cells, capability } => {
                    validate_cells(*node_id, *cells)?;
                    capability
                        .validate()
                        .map_err(|error| error.in_element(*node_id))?;
                }
                ElementNodeKind::Scalar { cells } => validate_cells(*node_id, *cells)?,
                ElementNodeKind::Indexed { cells, options } => {
                    validate_cells(*node_id, *cells)?;
                    if options.is_empty() {
                        return Err(ElementTreeValidationError::EmptyOptions(*node_id));
                    }
                    let mut ids = IndexSet::new();
                    for option in options {
                        if !ids.insert(option.id) {
                            return Err(ElementTreeValidationError::DuplicateOption {
                                node: *node_id,
                                option: option.id,
                            });
                        }
                    }
                }
                ElementNodeKind::Fixture { .. } => {}
            }
        }
        for root in &self.roots {
            if parent.contains_key(root) {
                return Err(ElementTreeValidationError::RootHasParent(*root));
            }
        }

        let mut visited = HashSet::new();
        let mut active = HashSet::new();
        for root in &self.roots {
            visit(self, *root, &mut visited, &mut active)?;
        }
        if let Some(node) = self.nodes.keys().find(|node| !visited.contains(node)) {
            return Err(ElementTreeValidationError::UnreachableNode(*node));
        }
        Ok(())
    }

    pub fn flatten_selection(
        &self,
        selection: &ElementSelection,
    ) -> Result<Vec<ElementCellAddress>, ElementSelectionError> {
        if selection.tree != self.id {
            return Err(ElementSelectionError::WrongTree);
        }
        let node = self
            .nodes
            .get(&selection.node)
            .ok_or(ElementSelectionError::MissingNode(selection.node))?;
        match &node.kind {
            ElementNodeKind::Group { .. } if selection.cells.is_some() => {
                Err(ElementSelectionError::GroupCellRange(selection.node))
            }
            ElementNodeKind::Group { .. } => {
                let mut output = Vec::new();
                flatten_node(self, selection.node, &mut output)?;
                Ok(output)
            }
            kind => range_addresses(
                selection.node,
                kind.cell_count().unwrap_or(0),
                selection.cells,
            ),
        }
    }

    pub fn flattened_cells(&self) -> Result<Vec<ElementCellAddress>, ElementSelectionError> {
        let mut cells = Vec::new();
        for root in &self.roots {
            flatten_node(self, *root, &mut cells)?;
        }
        Ok(cells)
    }
}

fn validate_cells(node: ElementNodeId, cells: u32) -> Result<(), ElementTreeValidationError> {
    if cells == 0 {
        Err(ElementTreeValidationError::EmptyCells(node))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ColorCapabilityValidationError {
    EmptyEmitters,
    EmptyMappings,
    DuplicateEmitter(EmitterId),
    DuplicateMappedColor(Color),
    MissingMappedEmitter(EmitterId),
    UnknownMappedEmitter(EmitterId),
    InvalidEmitterLevel(EmitterId),
}

impl ColorCapabilityValidationError {
    fn in_element(self, node: ElementNodeId) -> ElementTreeValidationError {
        match self {
            Self::EmptyEmitters => ElementTreeValidationError::EmptyEmitters(node),
            Self::EmptyMappings => ElementTreeValidationError::EmptyMappings(node),
            Self::DuplicateEmitter(emitter) => {
                ElementTreeValidationError::DuplicateEmitter { node, emitter }
            }
            Self::DuplicateMappedColor(color) => {
                ElementTreeValidationError::DuplicateMappedColor { node, color }
            }
            Self::MissingMappedEmitter(emitter) => {
                ElementTreeValidationError::MissingMappedEmitter { node, emitter }
            }
            Self::UnknownMappedEmitter(emitter) => {
                ElementTreeValidationError::UnknownMappedEmitter { node, emitter }
            }
            Self::InvalidEmitterLevel(emitter) => {
                ElementTreeValidationError::InvalidEmitterLevel { node, emitter }
            }
        }
    }
}

impl ColorCapability {
    pub fn validate(&self) -> Result<(), ColorCapabilityValidationError> {
        let ColorCapability::Discrete { emitters, mappings } = self else {
            return Ok(());
        };
        if emitters.is_empty() {
            return Err(ColorCapabilityValidationError::EmptyEmitters);
        }
        if mappings.is_empty() {
            return Err(ColorCapabilityValidationError::EmptyMappings);
        }
        let mut emitter_ids = IndexSet::new();
        for emitter in emitters {
            if !emitter_ids.insert(emitter.id) {
                return Err(ColorCapabilityValidationError::DuplicateEmitter(emitter.id));
            }
        }
        let mut mapped_colors = IndexSet::new();
        for mapping in mappings {
            if !mapped_colors.insert(mapping.color) {
                return Err(ColorCapabilityValidationError::DuplicateMappedColor(
                    mapping.color,
                ));
            }
            for emitter in &emitter_ids {
                if !mapping.levels.contains_key(emitter) {
                    return Err(ColorCapabilityValidationError::MissingMappedEmitter(
                        *emitter,
                    ));
                }
            }
            for (emitter, level) in &mapping.levels {
                if !emitter_ids.contains(emitter) {
                    return Err(ColorCapabilityValidationError::UnknownMappedEmitter(
                        *emitter,
                    ));
                }
                if !level.is_finite() || !(0.0..=1.0).contains(level) {
                    return Err(ColorCapabilityValidationError::InvalidEmitterLevel(
                        *emitter,
                    ));
                }
            }
        }
        Ok(())
    }
}

fn visit(
    tree: &ElementTree,
    node: ElementNodeId,
    visited: &mut HashSet<ElementNodeId>,
    active: &mut HashSet<ElementNodeId>,
) -> Result<(), ElementTreeValidationError> {
    if active.contains(&node) {
        return Err(ElementTreeValidationError::Cycle(node));
    }
    if !visited.insert(node) {
        return Ok(());
    }
    active.insert(node);
    let current = tree
        .nodes
        .get(&node)
        .ok_or(ElementTreeValidationError::MissingNode(node))?;
    if let ElementNodeKind::Group { children } = &current.kind {
        for child in children {
            visit(tree, *child, visited, active)?;
        }
    }
    active.remove(&node);
    Ok(())
}

fn flatten_node(
    tree: &ElementTree,
    node_id: ElementNodeId,
    output: &mut Vec<ElementCellAddress>,
) -> Result<(), ElementSelectionError> {
    let node = tree
        .nodes
        .get(&node_id)
        .ok_or(ElementSelectionError::MissingNode(node_id))?;
    match &node.kind {
        ElementNodeKind::Group { children } => {
            for child in children {
                flatten_node(tree, *child, output)?;
            }
        }
        kind => output.extend(range_addresses(
            node_id,
            kind.cell_count().unwrap_or(0),
            None,
        )?),
    }
    Ok(())
}

fn range_addresses(
    node: ElementNodeId,
    cell_count: u32,
    range: Option<ElementCellRange>,
) -> Result<Vec<ElementCellAddress>, ElementSelectionError> {
    let range = range.unwrap_or(ElementCellRange {
        start: 0,
        count: cell_count,
    });
    if range.count == 0 {
        return Err(ElementSelectionError::EmptyRange);
    }
    let Some(end) = range.start.checked_add(range.count) else {
        return Err(ElementSelectionError::RangeOutOfBounds { node, cell_count });
    };
    if end > cell_count {
        return Err(ElementSelectionError::RangeOutOfBounds { node, cell_count });
    }
    Ok((range.start..end)
        .map(|cell| ElementCellAddress { node, cell })
        .collect())
}
