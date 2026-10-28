//! The preparation shared by sequences and rigs: the fixtures and cells a
//! show keeps, the prepared targets its clips use, its layers, its global
//! signal graph and its output routes. Only how clips are placed differs.
use crate::selection::Selection;
use crate::sequence::programs::Programs;
use crate::sequence::{PreparationCache, composition};
use donder_language::compiler::{Invocation, ProgramConstants};
use donder_model::{
    AcceptedOperatorInputs, ClipScope, CompositionGraphNodeId, CompositionGraphNodeKind,
    FixtureInstanceId, FixtureTarget, PixelRouteId, SequenceCompositionGraph, SequenceLayer,
    SequenceLayerId, composition_graph_root_dependencies,
};
use donder_runtime::{ClipHandle, PreparedShow, ShowBuilder, TargetHandle};
use donder_runtime_types::{
    BoundProgram, FixtureGeometry, SequenceTiming, TargetGeometry, TargetScope,
};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

pub(crate) struct Stage {
    pub(crate) geometry: Vec<FixtureGeometry>,
    /// Every fixture and group's ordered member fixtures, by geometry index.
    pub(crate) targets: IndexMap<FixtureInstanceId, Vec<usize>>,
    /// Each selected route's pixels, in the original geometry.
    pub(crate) routes: IndexMap<PixelRouteId, TargetGeometry>,
    /// The cells of each fixture the show keeps.
    pub(crate) cells: Vec<Vec<u32>>,
    pub(crate) has_pixels: bool,
    pub(crate) dependencies: HashSet<CompositionGraphNodeId>,
    compact: bool,
}

/// What a show plays besides its clips: its layers and its graph.
pub(crate) struct Parts<'a> {
    pub(crate) layers: &'a [SequenceLayer],
    pub(crate) graph: &'a SequenceCompositionGraph,
    pub(crate) operators: Vec<AcceptedOperatorInputs<'a>>,
    pub(crate) timing: &'a SequenceTiming,
}

/// Build the show of `parts` on the selected setup, with the clips
/// `clips` places. Without `compact` every cell, layer and clip is kept;
/// with it, only what the selected outputs show.
pub(crate) fn build<'a, S>(
    selected: &Selection<'a, S>,
    parts: Parts<'a>,
    compact: bool,
    cache: &mut PreparationCache,
    clips: impl for<'id> FnOnce(&mut Clips<'_, 'id>),
) -> PreparedShow {
    let stage = Stage::new(selected, parts.graph, &parts.operators, compact);
    let required = stage.required_layers(parts.graph);
    // A compact show plays clips only in the enabled layers its output
    // depends on.
    let playing = parts
        .layers
        .iter()
        .filter(|layer| layer.enabled && required.contains(&layer.id))
        .map(|layer| &layer.id)
        .collect();
    PreparedShow::build(parts.timing.clone(), |builder| {
        for (index, (id, geometry)) in selected.geometry.iter().enumerate() {
            let geometry = if compact {
                geometry.select(|cell| stage.cells[index].binary_search(&cell).is_ok())
            } else {
                geometry.clone()
            };
            builder.fixture(id.0, geometry);
        }
        let mut placed = Clips {
            builder,
            stage: &stage,
            programs: Programs::new(cache),
            playing: &playing,
            targets: HashMap::new(),
            layers: parts
                .layers
                .iter()
                .map(|layer| (&layer.id, Vec::new()))
                .collect(),
        };
        clips(&mut placed);
        let Clips {
            builder,
            mut programs,
            layers: layer_clips,
            ..
        } = placed;
        let layers = parts
            .layers
            .iter()
            .filter(|layer| !compact || stage.has_pixels && required.contains(&layer.id))
            .map(|layer| {
                (
                    &layer.id,
                    builder.layer(layer.enabled, layer_clips[&layer.id].iter().copied()),
                )
            })
            .collect::<IndexMap<_, _>>();
        // A disabled or empty layer is black everywhere.
        let black = parts
            .layers
            .iter()
            .filter(|layer| !layer.enabled || layer_clips[&layer.id].is_empty())
            .map(|layer| &layer.id)
            .collect::<HashSet<_>>();
        let root = if compact && !stage.has_pixels {
            None
        } else {
            composition::prepare(
                builder,
                &parts,
                &stage.dependencies,
                &layers,
                &black,
                &mut programs,
            )
        };
        crate::sequence::prepare_routes(builder, selected, &stage);
        root
    })
}

/// Placing a show's clips: their targets, programs and layers.
pub(crate) struct Clips<'s, 'id> {
    pub(crate) builder: &'s mut ShowBuilder<'id>,
    stage: &'s Stage,
    programs: Programs<'s>,
    playing: &'s HashSet<&'s SequenceLayerId>,
    /// Clips on one target share its prepared geometry.
    targets: HashMap<(FixtureInstanceId, bool), TargetHandle<'id>>,
    layers: IndexMap<&'s SequenceLayerId, Vec<ClipHandle<'id>>>,
}

impl<'id> Clips<'_, 'id> {
    /// Whether the show leaves out a clip on `layer` and `target`: a
    /// compact show keeps only what its outputs show.
    pub(crate) fn drops(&self, layer: &SequenceLayerId, target: &FixtureTarget) -> bool {
        let stage = self.stage;
        stage.compact
            && (!stage.has_pixels
                || !self.playing.contains(layer)
                || stage.targets[&target.fixture]
                    .iter()
                    .all(|&index| stage.cells[index].is_empty()))
    }

    /// `target` in `scope`, and the pixel count its programs may assume.
    pub(crate) fn target(
        &mut self,
        target: &FixtureTarget,
        scope: &ClipScope,
    ) -> (TargetHandle<'id>, Option<i32>) {
        let scope = match scope {
            ClipScope::PerFixture => TargetScope::PerFixture,
            ClipScope::WholeTarget => TargetScope::WholeTarget,
        };
        let stage = self.stage;
        let members = &stage.targets[&target.fixture];
        let builder = &mut *self.builder;
        let handle = *self
            .targets
            .entry((target.fixture, scope == TargetScope::PerFixture))
            .or_insert_with(|| {
                builder.target(&TargetGeometry::new(
                    members.iter().map(|&index| (index, &stage.geometry[index])),
                    scope,
                ))
            });
        let mut counts = members
            .iter()
            .map(|&index| stage.geometry[index].positions().len() as i32);
        let pixel_count = match scope {
            TargetScope::WholeTarget => Some(counts.sum()),
            TargetScope::PerFixture => counts
                .next()
                .filter(|first| counts.all(|count| count == *first)),
        };
        (handle, pixel_count)
    }

    /// The program of `invocation` with `constants`.
    pub(crate) fn lower(
        &mut self,
        invocation: &Invocation,
        constants: ProgramConstants,
    ) -> BoundProgram {
        self.programs.clip(invocation, constants)
    }

    /// Play `clip` in `layer`.
    pub(crate) fn layer(&mut self, layer: &SequenceLayerId, clip: ClipHandle<'id>) {
        self.layers[layer].push(clip);
    }
}

impl Stage {
    fn new<S>(
        selected: &Selection<'_, S>,
        graph: &SequenceCompositionGraph,
        operators: &[AcceptedOperatorInputs<'_>],
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
        let dependencies = composition_graph_root_dependencies(graph);
        let cells =
            crate::sequence::retained_cells(operators, &geometry, &routes, &dependencies, compact);
        let has_pixels = cells.iter().any(|cells| !cells.is_empty());
        Self {
            geometry,
            targets,
            routes,
            cells,
            has_pixels,
            dependencies,
            compact,
        }
    }

    /// The layers the output depends on.
    fn required_layers<'g>(
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
}
