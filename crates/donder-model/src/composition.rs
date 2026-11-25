//! What sequences and consoles compose the same way: mark collections, layers
//! and the graph that draws the layers to the output.
use crate::effect::{Clip, ClipId};
use crate::operator::GraphOperatorNode;
use crate::project::{MemberMut, MemberRef};
use crate::site::Member;
use donder_language::{DonderDuration, DonderTime};
use donder_runtime_types::{Color, Identifier};
use std::sync::Arc;

/// A sequence's or console's mark collections, layers, graph and clips.
#[derive(Clone, Debug, PartialEq)]
pub struct Composition {
    pub marks: Vec<MarkCollection>,
    pub layers: Vec<Layer>,
    pub graph: CompositionGraph,
    /// Each clip is shared between project snapshots until an edit changes
    /// it, so an unchanged clip is the same allocation in both.
    pub clips: Vec<Arc<Clip>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LayerId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: LayerId,
    /// Unique among the sequence's layers and graph nodes.
    pub name: Identifier,
    pub description: Option<String>,
    pub color: Color,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompositionGraph {
    pub nodes: Vec<CompositionGraphNode>,
    pub edges: Vec<EffectGraphEdge>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompositionGraphNode {
    pub id: CompositionGraphNodeId,
    pub position: GraphNodePosition,
    pub kind: CompositionGraphNodeKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CompositionGraphNodeKind {
    Layer { layer_id: LayerId },
    Operator(GraphOperatorNode),
    Root,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct CompositionGraphNodeId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct GraphNodePosition {
    pub x: f32,
    pub y: f32,
}

/// Every node but the root has one output; an edge carries it to an input.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EffectGraphEdge {
    pub from: CompositionGraphNodeId,
    pub to: CompositionGraphNodeId,
    /// The operator input the edge feeds; `None` for the root's one input.
    pub input: Option<Identifier>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MarkCollection {
    pub id: MarkCollectionId,
    /// Unique among the sequence's mark collections; the text names it.
    pub name: Identifier,
    pub description: Option<String>,
    pub display_color: Color,
    pub marks: Vec<Mark>,
}

/// A moment or a span of time, optionally labeled: a beat, a lyric word, a
/// section. An instantaneous mark lasts zero.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub time: DonderTime,
    pub duration: DonderDuration,
    pub label: Option<String>,
}

impl Mark {
    /// An instantaneous, unlabeled mark.
    pub fn at(time: DonderTime) -> Self {
        Self {
            time,
            duration: DonderDuration(std::time::Duration::ZERO),
            label: None,
        }
    }

    /// When the mark ends.
    pub fn end(&self) -> std::time::Duration {
        self.time.0 + self.duration.0
    }
}

/// A mark collection of a sequence, or a live one of a console: unique among
/// its owner's, stable across renames.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct MarkCollectionId(pub u32);

impl Composition {
    /// A new sequence's or console's composition: no marks, and the default
    /// layer drawn straight to the output.
    pub fn default(color: Color) -> Self {
        let layer_id = LayerId(0);
        Self {
            marks: Vec::new(),
            layers: vec![Layer {
                id: layer_id.clone(),
                name: Identifier::new("default".to_string())
                    .unwrap_or_else(|_| unreachable!("literal names are valid")),
                description: None,
                color,
                enabled: true,
            }],
            graph: CompositionGraph {
                nodes: vec![
                    CompositionGraphNode {
                        id: CompositionGraphNodeId(1),
                        position: GraphNodePosition { x: 80.0, y: 80.0 },
                        kind: CompositionGraphNodeKind::Layer { layer_id },
                    },
                    CompositionGraphNode {
                        id: CompositionGraphNodeId(2),
                        position: GraphNodePosition { x: 420.0, y: 80.0 },
                        kind: CompositionGraphNodeKind::Root,
                    },
                ],
                edges: vec![EffectGraphEdge {
                    from: CompositionGraphNodeId(1),
                    to: CompositionGraphNodeId(2),
                    input: None,
                }],
            },
            clips: Vec::new(),
        }
    }

    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.clips
            .iter()
            .find(|clip| clip.id == id)
            .map(Arc::as_ref)
    }

    /// The mark collection, layer, graph node or clip `member` names.
    pub(crate) fn member(&self, member: &Member) -> Option<MemberRef<'_>> {
        match member {
            Member::Clip(id) => self.clip(*id).map(MemberRef::Clip),
            Member::Marks(id) => self
                .marks
                .iter()
                .find(|marks| &marks.id == id)
                .map(MemberRef::Marks),
            Member::Layer(id) => self
                .layers
                .iter()
                .find(|layer| &layer.id == id)
                .map(MemberRef::Layer),
            Member::Node(id) => self
                .graph
                .nodes
                .iter()
                .find(|node| &node.id == id)
                .map(MemberRef::Node),
            _ => None,
        }
    }

    pub(crate) fn member_mut(&mut self, member: &Member) -> Option<MemberMut<'_>> {
        match member {
            Member::Clip(id) => self
                .clips
                .iter_mut()
                .find(|clip| clip.id == *id)
                .map(MemberMut::Clip),
            Member::Marks(id) => self
                .marks
                .iter_mut()
                .find(|marks| &marks.id == id)
                .map(MemberMut::Marks),
            Member::Layer(id) => self
                .layers
                .iter_mut()
                .find(|layer| &layer.id == id)
                .map(MemberMut::Layer),
            Member::Node(id) => self
                .graph
                .nodes
                .iter_mut()
                .find(|node| &node.id == id)
                .map(MemberMut::Node),
            _ => None,
        }
    }

    /// Remove the member; whether it was there.
    pub(crate) fn remove(&mut self, member: &Member) -> bool {
        fn retain<T>(items: &mut Vec<T>, keep: impl FnMut(&T) -> bool) -> bool {
            let count = items.len();
            items.retain(keep);
            items.len() != count
        }
        match member {
            Member::Clip(id) => retain(&mut self.clips, |clip| clip.id != *id),
            Member::Marks(id) => retain(&mut self.marks, |marks| &marks.id != id),
            Member::Layer(id) => retain(&mut self.layers, |layer| &layer.id != id),
            Member::Node(id) => retain(&mut self.graph.nodes, |node| &node.id != id),
            _ => false,
        }
    }
}
