//! The active setup's side of preparation, shared by sequences and rigs: the
//! fixtures and cells a show keeps, and the prepared targets its effects use.
use crate::selection::Selection;
use donder_model::{
    AcceptedOperatorInputs, CompositionGraphNodeId, CompositionGraphNodeKind, EffectScope,
    FixtureInstanceId, FixtureTarget, SequenceCompositionGraph, SequenceLayerId,
    composition_graph_output_dependencies,
};
use donder_runtime::{FixtureHandle, SequenceBuilder, TargetHandle};
use donder_runtime_types::{FixtureGeometry, TargetScope};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

pub(crate) struct Stage {
    pub(crate) geometry: Vec<FixtureGeometry>,
    /// Every fixture and group's ordered member fixtures, by geometry index.
    pub(crate) targets: IndexMap<FixtureInstanceId, Vec<usize>>,
    /// The cells of each fixture the show keeps.
    pub(crate) cells: Vec<Vec<u32>>,
    pub(crate) has_pixels: bool,
    pub(crate) dependencies: HashSet<CompositionGraphNodeId>,
    compact: bool,
}

impl Stage {
    pub(crate) fn new<'a, S>(
        selected: &Selection<'_, S>,
        graph: &SequenceCompositionGraph,
        operators: impl Iterator<Item = AcceptedOperatorInputs<'a>>,
        compact: bool,
    ) -> Self {
        let geometry = selected
            .geometry
            .iter()
            .map(|(_, geometry)| geometry.clone())
            .collect::<Vec<_>>();
        let indexes = selected
            .geometry
            .iter()
            .enumerate()
            .map(|(index, (id, _))| (*id, index))
            .collect::<IndexMap<_, _>>();
        // Every group denotes its ordered member fixtures, including empty groups.
        let targets = selected
            .layout
            .iter_fixtures()
            .map(|item| {
                let members = selected.layout.members(item.id);
                (item.id, members.iter().map(|id| indexes[id]).collect())
            })
            .collect::<IndexMap<FixtureInstanceId, Vec<usize>>>();
        let routes = crate::sequence::route_targets(selected, &geometry, &targets);
        let dependencies = composition_graph_output_dependencies(graph);
        let cells =
            crate::sequence::retained_cells(operators, &geometry, &routes, &dependencies, compact);
        let has_pixels = cells.iter().any(|cells| !cells.is_empty());
        Self {
            geometry,
            targets,
            cells,
            has_pixels,
            dependencies,
            compact,
        }
    }

    /// The layers the output depends on.
    pub(crate) fn required_layers<'g>(
        &self,
        graph: &'g SequenceCompositionGraph,
    ) -> HashSet<&'g SequenceLayerId> {
        graph
            .nodes
            .iter()
            .filter_map(|node| {
                if self.dependencies.contains(&node.id)
                    && let CompositionGraphNodeKind::Layer { layer_id } = &node.kind
                {
                    Some(layer_id)
                } else {
                    None
                }
            })
            .collect()
    }

    pub(crate) fn fixtures<'id, S>(
        &self,
        builder: &mut SequenceBuilder<'id>,
        selected: &Selection<'_, S>,
    ) -> IndexMap<usize, FixtureHandle<'id>> {
        selected
            .geometry
            .iter()
            .enumerate()
            .map(|(index, (id, geometry))| {
                let geometry = if self.compact {
                    geometry.select(|cell| self.cells[index].binary_search(&cell).is_ok())
                } else {
                    geometry.clone()
                };
                (index, builder.fixture(id.0, geometry))
            })
            .collect()
    }

    /// Whether a compact preparation keeps no pixel of `target`.
    pub(crate) fn drops(&self, target: &FixtureTarget) -> bool {
        self.compact
            && self.targets[&target.fixture]
                .iter()
                .all(|&index| self.cells[index].is_empty())
    }
}

/// The prepared targets of a show's effects; effects on one target share its
/// prepared geometry.
#[derive(Default)]
pub(crate) struct Targets<'id> {
    prepared: HashMap<(FixtureInstanceId, bool), TargetHandle<'id>>,
}

impl<'id> Targets<'id> {
    /// `target` in `scope`, and the pixel count its programs may assume.
    pub(crate) fn get(
        &mut self,
        builder: &mut SequenceBuilder<'id>,
        stage: &Stage,
        fixtures: &IndexMap<usize, FixtureHandle<'id>>,
        target: &FixtureTarget,
        scope: &EffectScope,
    ) -> (TargetHandle<'id>, Option<i32>) {
        let runtime_scope = match scope {
            EffectScope::PerFixture => TargetScope::PerFixture,
            EffectScope::WholeTarget => TargetScope::WholeTarget,
        };
        let members = &stage.targets[&target.fixture];
        let handle = *self
            .prepared
            .entry((target.fixture, *scope == EffectScope::PerFixture))
            .or_insert_with(|| {
                builder.target(members.iter().map(|index| fixtures[index]), runtime_scope)
            });
        let mut counts = members
            .iter()
            .map(|&index| stage.geometry[index].positions().len() as i32);
        let pixel_count = match runtime_scope {
            TargetScope::WholeTarget => Some(counts.sum()),
            TargetScope::PerFixture => counts
                .next()
                .filter(|first| counts.all(|count| count == *first)),
        };
        (handle, pixel_count)
    }
}
