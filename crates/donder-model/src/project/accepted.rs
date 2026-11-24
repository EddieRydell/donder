use super::DonderProject;
use crate::effect::Clip;
use crate::operator::OperatorDefinition;
use crate::sequence::{CompositionGraphNode, CompositionGraphNodeKind, Sequence, SequenceId};
mod automation;
mod console;
mod geometry;
mod inputs;
mod parameters;
pub(super) use parameters::{MarkOrigin, derivations};
mod patch;
use crate::console::{ClipUse, Console, ConsoleClip, ConsoleId};
pub use console::{CONSOLE_SESSION, ConsoleBinding};
use donder_language::compiler::Invocation;
pub(super) use inputs::ProjectInputs;
pub use patch::PlaybackRoute;

/// A sequence borrowed from the same accepted project as all its dependencies.
#[derive(Clone, Copy)]
pub struct AcceptedSequence<'a> {
    project: &'a DonderProject,
    sequence: &'a Sequence,
}

/// An authored clip paired with its admitted playback invocation.
#[derive(Clone, Copy)]
pub struct AcceptedClipInputs<'a> {
    clip: &'a Clip,
    invocation: &'a Invocation,
}

#[derive(Clone, Copy)]
pub struct AcceptedOperatorInputs<'a> {
    node: &'a CompositionGraphNode,
    definition: &'a OperatorDefinition,
    invocation: &'a Invocation,
}

impl<'p> crate::View<'p, crate::patch::Patch> {
    /// Encodings and placements belong to this accepted patch snapshot; its
    /// authored route IDs index them without readmission.
    pub fn playback_routes(
        &self,
    ) -> &'p indexmap::IndexMap<crate::patch::PixelRouteId, PlaybackRoute> {
        &self.project().accepted_inputs.patches[&self.id()].routes
    }
}

impl<'p> crate::View<'p, crate::layout::Layout> {
    /// The admitted geometry of each fixture of the layout.
    pub fn playback_geometry(
        &self,
    ) -> &'p [(
        crate::layout::FixtureInstanceId,
        donder_runtime_types::FixtureGeometry,
    )] {
        &self.project().accepted_inputs.layouts[&self.id()]
    }
}

impl DonderProject {
    pub fn accepted_console(&self, id: &ConsoleId) -> Option<AcceptedConsole<'_>> {
        self.get::<crate::Console>(*id)
            .map(|console| AcceptedConsole {
                project: self,
                console,
            })
    }

    pub fn accepted_sequence(&self, id: &SequenceId) -> Option<AcceptedSequence<'_>> {
        self.get::<crate::Sequence>(*id)
            .map(|sequence| AcceptedSequence {
                project: self,
                sequence,
            })
    }
}

impl<'a> AcceptedSequence<'a> {
    pub fn timing(self) -> &'a donder_runtime_types::SequenceTiming {
        &self.project.accepted_inputs.sequences[&self.sequence.id].timing
    }

    pub fn sequence(self) -> &'a Sequence {
        self.sequence
    }

    pub fn clips(self) -> impl ExactSizeIterator<Item = AcceptedClipInputs<'a>> + 'a {
        self.sequence
            .clips
            .iter()
            .zip(
                self.project.accepted_inputs.sequences[&self.sequence.id]
                    .clips
                    .iter(),
            )
            .map(|(clip, invocation)| AcceptedClipInputs {
                clip,
                invocation: invocation.as_ref(),
            })
    }

    pub fn operators(self) -> impl Iterator<Item = AcceptedOperatorInputs<'a>> + 'a {
        operator_inputs(
            self.project,
            &self.sequence.composition_graph,
            &self.project.accepted_inputs.sequences[&self.sequence.id].operators,
        )
    }
}

/// A console borrowed from the same accepted project as all its dependencies.
#[derive(Clone, Copy)]
pub struct AcceptedConsole<'a> {
    project: &'a DonderProject,
    console: &'a Console,
}

#[derive(Clone, Copy)]
pub struct AcceptedConsoleClip<'a> {
    clip: &'a ConsoleClip,
    clip_use: ClipUse,
    inputs: &'a console::ConsoleClipInputs,
}

impl<'a> AcceptedConsole<'a> {
    pub fn console(self) -> &'a Console {
        self.console
    }

    /// The session clock: the whole playback range, with no authored windows.
    pub fn timing(self) -> &'a donder_runtime_types::SequenceTiming {
        &self.project.accepted_inputs.consoles[&self.console.id].timing
    }

    /// The console's clips, in authored order.
    pub fn clips(self) -> impl Iterator<Item = AcceptedConsoleClip<'a>> + 'a {
        self.console
            .clips
            .iter()
            .zip(
                self.project.accepted_inputs.consoles[&self.console.id]
                    .clips
                    .iter(),
            )
            .map(move |(clip, inputs)| AcceptedConsoleClip {
                clip,
                clip_use: self.console.clip_use(clip.id),
                inputs,
            })
    }

    pub fn operators(self) -> impl Iterator<Item = AcceptedOperatorInputs<'a>> + 'a {
        operator_inputs(
            self.project,
            &self.console.composition_graph,
            &self.project.accepted_inputs.consoles[&self.console.id].operators,
        )
    }

    /// The index of the console's live automation that sets each automation
    /// binding of operator `node`'s invocation, in binding order.
    pub fn operator_automations(
        self,
        node: &crate::sequence::CompositionGraphNodeId,
    ) -> &'a [usize] {
        &self.project.accepted_inputs.consoles[&self.console.id].operator_automations[node]
    }
}

impl<'a> AcceptedConsoleClip<'a> {
    pub fn clip(self) -> &'a ConsoleClip {
        self.clip
    }

    /// Whether `Play` actions spawn copies of it, or one copy plays held.
    pub fn clip_use(self) -> ClipUse {
        self.clip_use
    }

    pub fn invocation(self) -> &'a Invocation {
        &self.inputs.invocation
    }

    /// What sets each of the invocation's automation bindings.
    pub fn bindings(self) -> &'a [ConsoleBinding] {
        &self.inputs.bindings
    }
}

fn operator_inputs<'a>(
    project: &'a DonderProject,
    graph: &'a crate::sequence::SequenceCompositionGraph,
    invocations: &'a indexmap::IndexMap<crate::sequence::CompositionGraphNodeId, Invocation>,
) -> impl Iterator<Item = AcceptedOperatorInputs<'a>> + 'a {
    graph.nodes.iter().filter_map(move |node| {
        let CompositionGraphNodeKind::Operator(operator) = &node.kind else {
            return None;
        };
        let id = &operator.operator;
        Some(AcceptedOperatorInputs {
            node,
            definition: &project.reusable::<crate::OperatorDefinition>()[id],
            invocation: &invocations[&node.id],
        })
    })
}

impl<'a> AcceptedClipInputs<'a> {
    pub fn invocation(self) -> &'a Invocation {
        self.invocation
    }

    pub fn clip(self) -> &'a Clip {
        self.clip
    }

    /// When the clip starts on the sequence clock; admission checked that it
    /// plays within the sequence.
    pub fn start(self) -> donder_runtime_types::SampleTime {
        crate::clock::sample_time(&self.clip.start)
    }

    pub fn duration(self) -> donder_runtime_types::SampleDuration {
        crate::clock::sample_duration(&self.clip.duration)
    }
}

impl<'a> AcceptedOperatorInputs<'a> {
    pub fn invocation(self) -> &'a Invocation {
        self.invocation
    }

    pub fn node(self) -> &'a CompositionGraphNode {
        self.node
    }
    pub fn definition(self) -> &'a OperatorDefinition {
        self.definition
    }
}
