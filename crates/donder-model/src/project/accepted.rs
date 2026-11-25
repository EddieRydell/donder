use super::DonderProject;
use crate::composition::{
    Composition, CompositionGraph, CompositionGraphNode, CompositionGraphNodeId,
    CompositionGraphNodeKind,
};
use crate::console::{Console, ConsoleId};
use crate::effect::{Clip, ScriptDefinition};
use crate::sequence::{Sequence, SequenceId};
mod automation;
mod console;
mod geometry;
mod inputs;
mod parameters;
pub(super) use parameters::{MarkOrigin, derivations};
mod patch;
pub use console::LiveBinding;
use donder_language::compiler::Invocation;
pub(super) use inputs::ProjectInputs;
pub use patch::PlaybackRoute;

/// What plays: a sequence, or a console.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ShowId {
    Sequence(SequenceId),
    Console(ConsoleId),
}

/// A show's playback inputs, admitted with the project.
#[derive(Debug)]
pub(super) struct CompositionInputs {
    pub(super) timing: donder_runtime_types::SequenceTiming,
    /// Of the effect clips, in the composition's order.
    pub(super) clips: Box<[std::sync::Arc<ClipInputs>]>,
    pub(super) operators: indexmap::IndexMap<CompositionGraphNodeId, Invocation>,
    /// The console's set parameter that sets each of an operator's
    /// automation bindings, in binding order; none in a sequence.
    pub(super) operator_automations: indexmap::IndexMap<CompositionGraphNodeId, Box<[usize]>>,
}

#[derive(Debug)]
pub(super) struct ClipInputs {
    pub(super) invocation: Invocation,
    /// What sets each of the invocation's live automation bindings, which
    /// follow its authored ones; none in a sequence.
    pub(super) live: Box<[LiveBinding]>,
}

/// A sequence or console borrowed from the same accepted project as all its
/// dependencies.
#[derive(Clone, Copy)]
pub struct AcceptedShow<'a> {
    project: &'a DonderProject,
    show: Show<'a>,
    inputs: &'a CompositionInputs,
}

#[derive(Clone, Copy)]
enum Show<'a> {
    Sequence(&'a Sequence),
    Console(&'a Console),
}

/// An authored clip paired with its admitted playback invocation.
#[derive(Clone, Copy)]
pub struct AcceptedClip<'a> {
    clip: &'a Clip,
    inputs: &'a ClipInputs,
}

#[derive(Clone, Copy)]
pub struct AcceptedOperatorInputs<'a> {
    node: &'a CompositionGraphNode,
    definition: &'a ScriptDefinition,
    invocation: &'a Invocation,
}

impl DonderProject {
    /// Encodings and placements belong to this accepted snapshot; the
    /// authored route IDs index them without readmission.
    pub fn playback_routes(
        &self,
    ) -> &indexmap::IndexMap<crate::patch::PixelRouteId, PlaybackRoute> {
        &self.accepted_inputs.routes.routes
    }

    pub fn accepted(&self, id: ShowId) -> Option<AcceptedShow<'_>> {
        let show = match id {
            ShowId::Sequence(id) => Show::Sequence(self.get::<Sequence>(id)?),
            ShowId::Console(id) => Show::Console(self.get::<Console>(id)?),
        };
        Some(AcceptedShow {
            project: self,
            show,
            inputs: &self.accepted_inputs.shows[&id],
        })
    }
}

impl<'p> crate::View<'p, crate::layout::Layout> {
    /// The admitted geometry of each fixture of the layout.
    pub fn playback_geometry(
        &self,
    ) -> &'p [(
        crate::layout::FixtureId,
        donder_runtime_types::FixtureGeometry,
    )] {
        &self.project().accepted_inputs.layouts[&self.id()]
    }
}

impl<'a> AcceptedShow<'a> {
    /// The sequence's clock, or a console's session clock: the whole playback
    /// range.
    pub fn timing(self) -> &'a donder_runtime_types::SequenceTiming {
        &self.inputs.timing
    }

    pub fn composition(self) -> &'a Composition {
        match self.show {
            Show::Sequence(sequence) => &sequence.composition,
            Show::Console(console) => &console.composition,
        }
    }

    pub fn console(self) -> Option<&'a Console> {
        match self.show {
            Show::Console(console) => Some(console),
            Show::Sequence(_) => None,
        }
    }

    /// The composition's effect clips, in authored order.
    pub fn clips(self) -> impl Iterator<Item = AcceptedClip<'a>> + 'a {
        self.composition()
            .clips
            .iter()
            .filter(|clip| clip.effect().is_some())
            .zip(self.inputs.clips.iter())
            .map(|(clip, inputs)| AcceptedClip { clip, inputs })
    }

    pub fn operators(self) -> impl Iterator<Item = AcceptedOperatorInputs<'a>> + 'a {
        operator_inputs(
            self.project,
            &self.composition().graph,
            &self.inputs.operators,
        )
    }

    /// The index of the console's live automation that sets each automation
    /// binding of operator `node`'s invocation, in binding order.
    pub fn operator_automations(self, node: &CompositionGraphNodeId) -> &'a [usize] {
        self.inputs
            .operator_automations
            .get(node)
            .map_or(&[], |automations| automations)
    }
}

impl<'a> AcceptedClip<'a> {
    pub fn clip(self) -> &'a Clip {
        self.clip
    }

    /// The effect it plays.
    pub fn effect(self) -> &'a crate::effect::Effect {
        self.clip
            .effect()
            .unwrap_or_else(|| unreachable!("an accepted clip plays an effect"))
    }

    pub fn invocation(self) -> &'a Invocation {
        &self.inputs.invocation
    }

    /// What sets each of the invocation's live bindings.
    pub fn live(self) -> &'a [LiveBinding] {
        &self.inputs.live
    }

    /// When a timed clip starts and how long it plays on its show's clock;
    /// admission checked that it plays within it.
    pub fn timing(
        self,
    ) -> Option<(
        donder_runtime_types::SampleTime,
        donder_runtime_types::SampleDuration,
    )> {
        self.clip.timing().map(|(start, duration)| {
            (
                crate::clock::sample_time(start),
                crate::clock::sample_duration(duration),
            )
        })
    }
}

fn operator_inputs<'a>(
    project: &'a DonderProject,
    graph: &'a CompositionGraph,
    invocations: &'a indexmap::IndexMap<CompositionGraphNodeId, Invocation>,
) -> impl Iterator<Item = AcceptedOperatorInputs<'a>> + 'a {
    graph.nodes.iter().filter_map(move |node| {
        let CompositionGraphNodeKind::Operator(operator) = &node.kind else {
            return None;
        };
        let id = &operator.operator;
        Some(AcceptedOperatorInputs {
            node,
            definition: &project.reusable::<crate::ScriptDefinition>()[id],
            invocation: &invocations[&node.id],
        })
    })
}

impl<'a> AcceptedOperatorInputs<'a> {
    pub fn invocation(self) -> &'a Invocation {
        self.invocation
    }

    pub fn node(self) -> &'a CompositionGraphNode {
        self.node
    }
    pub fn definition(self) -> &'a ScriptDefinition {
        self.definition
    }
}
