//! The preparation shared by sequences and consoles: the fixtures and cells a
//! show keeps, the prepared targets its clips use, its layers, its global
//! signal graph and its output routes.
use crate::selection::Selection;
use crate::sequence::programs::Programs;
use crate::sequence::{PreparationCache, composition};
use donder_language::compiler::{Invocation, LiveWords, ProgramConstants};
use donder_model::{
    AcceptedOperatorInputs, CompositionGraph, CompositionGraphNodeId, CompositionGraphNodeKind,
    FixtureId, LayerId, PixelRouteId, composition_graph_root_dependencies,
};
use donder_runtime::{ClipHandle, PreparedShow, ShowBuilder, TargetHandle};
use donder_runtime_types::{
    BoundProgram, FixtureGeometry, TargetGeometry, TargetScope, sample_duration_seconds_f32,
};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

pub(crate) struct Stage {
    geometry: Vec<FixtureGeometry>,
    /// Every fixture and group's ordered parts: a fixture's geometry index
    /// and the run of its cells the target selects.
    targets: IndexMap<FixtureId, Vec<(usize, Range<u32>)>>,
    /// Each selected route's pixels, in the original geometry.
    pub(crate) routes: IndexMap<PixelRouteId, TargetGeometry>,
    /// The cells of each fixture the show keeps.
    cells: Vec<Vec<u32>>,
    has_pixels: bool,
    dependencies: HashSet<CompositionGraphNodeId>,
    compact: bool,
}

/// Build the show `selected` plays on the project's layout and controllers:
/// its timed clips each at its time, and, for a console, with `route`, its
/// played clips' dormant voices and the routing of its inputs. Without
/// `compact` every cell, layer and clip is kept; with it, only what the
/// selected outputs show.
pub(crate) fn build(
    selected: &Selection<'_>,
    compact: bool,
    cache: &mut PreparationCache,
    route: Option<&crate::console::RoutingPlan<'_>>,
) -> PreparedShow {
    let show = selected.show;
    let composition = show.composition();
    let operators = show.operators().collect::<Vec<_>>();
    let stage = Stage::new(selected, &composition.graph, &operators, compact);
    let required = stage.required_layers(&composition.graph);
    // A compact show plays clips only in the enabled layers its output
    // depends on.
    let playing = composition
        .layers
        .iter()
        .filter(|layer| layer.enabled && required.contains(&layer.id))
        .map(|layer| &layer.id)
        .collect();
    PreparedShow::build(show.timing().clone(), |builder| {
        for (index, (id, geometry)) in selected.geometry.iter().enumerate() {
            let geometry = if compact {
                geometry.select(|cell| stage.cells[index].binary_search(&cell).is_ok())
            } else {
                geometry.clone()
            };
            builder.fixture(id.0, geometry);
        }
        let mut clips = Clips {
            builder,
            stage: &stage,
            programs: Programs::new(cache),
            playing: &playing,
            targets: HashMap::new(),
            layers: composition
                .layers
                .iter()
                .map(|layer| (&layer.id, Vec::new()))
                .collect(),
        };
        let mut live = route.map(|route| crate::console::Live::new(route, clips.builder));
        for clip in show.clips() {
            let authored = clip.clip();
            let effect = clip.effect();
            // A dropped played clip keeps its voices, so commands still
            // address them, but plays in no layer.
            let dropped = clips.drops(&effect.layer, authored.target);
            let timing = clip.timing();
            if dropped && timing.is_some() {
                continue;
            }
            let (target, pixel_count) = clips.target(authored.target, effect.scope);
            let duration = match (timing, &route) {
                (Some((_, duration)), _) => Some(duration),
                (None, Some(route)) => route.fixed_length(authored.id),
                (None, None) => unreachable!("validation keeps a sequence's clips timed"),
            };
            let constants = ProgramConstants {
                pixel_count,
                duration_seconds: duration.map(sample_duration_seconds_f32),
            };
            match (timing, &mut live) {
                (Some((start, duration)), live) => {
                    let (words, snapshots) = match live {
                        Some(live) => live.words(clip, &[], clips.builder),
                        None => Default::default(),
                    };
                    let program = clips.lower(clip.invocation(), constants, &words);
                    let handle =
                        clips
                            .builder
                            .clip(Some(authored.id.0), &program, start, duration, target);
                    for (automation, word) in snapshots {
                        clips.builder.snapshot(handle, automation, word);
                    }
                    if !dropped {
                        clips.layer(&effect.layer, handle);
                    }
                }
                (None, Some(live)) => live.voices(&mut clips, clip, constants, target, dropped),
                (None, None) => unreachable!("validation keeps a sequence's clips timed"),
            }
        }
        let Clips {
            builder,
            mut programs,
            layers: layer_clips,
            ..
        } = clips;
        let layers = composition
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
        let black = composition
            .layers
            .iter()
            .filter(|layer| !layer.enabled || layer_clips[&layer.id].is_empty())
            .map(|layer| &layer.id)
            .collect::<HashSet<_>>();
        let operator_words = |node: &donder_model::CompositionGraphNodeId| {
            live.as_ref()
                .map_or_else(Vec::new, |live| live.operator_words(show, node))
        };
        let root = if compact && !stage.has_pixels {
            None
        } else {
            composition::prepare(
                builder,
                show,
                &stage.dependencies,
                &layers,
                &black,
                &mut programs,
                &operator_words,
            )
        };
        if let Some(live) = live {
            live.route(builder);
        }
        crate::sequence::prepare_routes(builder, selected, &stage);
        root
    })
}

/// Placing a show's clips: their targets, programs and layers.
pub(crate) struct Clips<'s, 'id> {
    pub(crate) builder: &'s mut ShowBuilder<'id>,
    stage: &'s Stage,
    programs: Programs<'s>,
    playing: &'s HashSet<&'s LayerId>,
    /// Clips on one target share its prepared geometry.
    targets: HashMap<(FixtureId, bool), TargetHandle<'id>>,
    layers: IndexMap<&'s LayerId, Vec<ClipHandle<'id>>>,
}

impl<'id> Clips<'_, 'id> {
    /// Whether the show leaves out a clip on `layer` and `target`: a
    /// compact show keeps only what its outputs show.
    pub(crate) fn drops(&self, layer: &LayerId, target: FixtureId) -> bool {
        let stage = self.stage;
        stage.compact
            && (!stage.has_pixels
                || !self.playing.contains(layer)
                || stage.targets[&target]
                    .iter()
                    .all(|(index, _)| stage.cells[*index].is_empty()))
    }

    /// `target` in `scope`, and the pixel count its programs may assume.
    pub(crate) fn target(
        &mut self,
        target: FixtureId,
        scope: TargetScope,
    ) -> (TargetHandle<'id>, Option<i32>) {
        let stage = self.stage;
        let members = &stage.targets[&target];
        let builder = &mut *self.builder;
        let handle = *self
            .targets
            .entry((target, scope == TargetScope::PerFixture))
            .or_insert_with(|| {
                builder.target(&TargetGeometry::parts(
                    members
                        .iter()
                        .map(|(index, cells)| (*index, &stage.geometry[*index], cells.clone())),
                    scope,
                ))
            });
        let mut counts = members.iter().map(|(_, cells)| cells.len() as i32);
        let pixel_count = match scope {
            TargetScope::WholeTarget => Some(counts.sum()),
            TargetScope::PerFixture => counts
                .next()
                .filter(|first| counts.all(|count| count == *first)),
        };
        (handle, pixel_count)
    }

    /// The program of `invocation` with `constants`, its live bindings
    /// reading the words `live` names.
    pub(crate) fn lower(
        &mut self,
        invocation: &Invocation,
        constants: ProgramConstants,
        live: &[LiveWords],
    ) -> BoundProgram {
        self.programs.clip(invocation, constants, live)
    }

    /// Play `clip` in `layer`.
    pub(crate) fn layer(&mut self, layer: &LayerId, clip: ClipHandle<'id>) {
        self.layers[layer].push(clip);
    }
}

impl Stage {
    fn new(
        selected: &Selection<'_>,
        graph: &CompositionGraph,
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
        // Every group denotes its ordered parts, including empty groups.
        let targets = selected
            .layout
            .iter_fixtures()
            .map(|item| {
                let parts = selected.layout.members(item.id);
                let parts = parts
                    .iter()
                    .map(|part| {
                        let index = indexes[&part.fixture];
                        let count = geometry[index].positions().len() as u32;
                        let cells = part.pixels.map_or(0..count, |span| {
                            span.start..span.start.saturating_add(span.count)
                        });
                        (index, cells)
                    })
                    .collect();
                (item.id, parts)
            })
            .collect::<IndexMap<FixtureId, Vec<(usize, Range<u32>)>>>();
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
    fn required_layers<'g>(&self, graph: &'g CompositionGraph) -> HashSet<&'g LayerId> {
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
