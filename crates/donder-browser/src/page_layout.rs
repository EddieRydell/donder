use super::*;
use donder_model::AutomationTarget;
use donder_sequence_api::BrowserPageNode;
use std::collections::{HashMap, HashSet};

const MAX_PAGE_PIXELS: usize = 100_000;
const MAX_PAGE_COORDINATE: f32 = 1_000.0;

pub(super) fn validate_page(page: &[BrowserPageNode]) -> Result<(), JsValue> {
    let mut ids = HashSet::new();
    let mut pixels = 0;
    validate_nodes(page, &mut ids, &mut pixels)
}

fn validate_nodes(
    nodes: &[BrowserPageNode],
    ids: &mut HashSet<u32>,
    pixels: &mut usize,
) -> Result<(), JsValue> {
    for node in nodes {
        let (id, name) = match node {
            BrowserPageNode::Group { id, name, children } => {
                validate_nodes(children, ids, pixels)?;
                (id, name)
            }
            BrowserPageNode::Fixture {
                id,
                name,
                pixels: points,
            } => {
                if points.is_empty() {
                    return Err(JsValue::from_str(&format!(
                        "Page fixture {name} has no pixels."
                    )));
                }
                if points.iter().flatten().any(|coordinate| {
                    !coordinate.is_finite() || coordinate.abs() > MAX_PAGE_COORDINATE
                }) {
                    return Err(JsValue::from_str(&format!(
                        "Page fixture {name} has a coordinate outside +/-{MAX_PAGE_COORDINATE} m."
                    )));
                }
                *pixels += points.len();
                (id, name)
            }
        };
        if !ids.insert(*id) {
            return Err(JsValue::from_str(&format!(
                "Page node id {id} ({name}) is used more than once."
            )));
        }
    }
    if *pixels > MAX_PAGE_PIXELS {
        return Err(JsValue::from_str(&format!(
            "The page has more than {MAX_PAGE_PIXELS} pixels."
        )));
    }
    Ok(())
}

pub(super) fn page_layout(id: LayoutId, page: &[BrowserPageNode]) -> Layout {
    // The page is a tree: every node is listed once, under its parent.
    fn push(node: &BrowserPageNode, fixtures: &mut Vec<LayoutFixture>) -> FixtureInstanceId {
        let fixture = match node {
            BrowserPageNode::Group { id, name, children } => LayoutFixture {
                description: None,
                id: FixtureInstanceId(*id),
                name: donder_language::object_name(name),
                kind: LayoutFixtureKind::Group {
                    members: children.iter().map(node_id).collect(),
                },
            },
            BrowserPageNode::Fixture { id, name, pixels } => LayoutFixture {
                description: None,
                id: FixtureInstanceId(*id),
                name: donder_language::object_name(name),
                kind: LayoutFixtureKind::Fixture {
                    definition: ValueSource::Inline(fixture_definition(pixels)),
                    transform: FixtureTransform::default(),
                },
            },
        };
        let id = fixture.id;
        fixtures.push(fixture);
        if let BrowserPageNode::Group { children, .. } = node {
            for child in children {
                push(child, fixtures);
            }
        }
        id
    }
    let mut fixtures = Vec::new();
    let root = page.iter().map(|node| push(node, &mut fixtures)).collect();
    Layout {
        description: None,
        id,
        fixtures,
        root,
    }
}

fn node_id(node: &BrowserPageNode) -> FixtureInstanceId {
    match node {
        BrowserPageNode::Group { id, .. } | BrowserPageNode::Fixture { id, .. } => {
            FixtureInstanceId(*id)
        }
    }
}

fn fixture_definition(pixels: &[[f32; 2]]) -> FixtureDefinition {
    FixtureDefinition {
        description: None,
        elements: pixels
            .iter()
            .enumerate()
            .map(|(index, [x, y])| FixtureElement {
                id: FixtureElementId(index as u32 + 1),
                name: donder_language::object_name(&format!("pixel_{}", index + 1)),
                transform: FixtureTransform {
                    position: Point3 {
                        x: Distance::from_meters(*x),
                        y: Distance::from_meters(*y),
                        z: Distance::ZERO,
                    },
                    ..FixtureTransform::default()
                },
                diameter: DistanceSpan::from_meters(0.01),
                reverse: false,
                shape: FixtureShape::Pixel,
            })
            .collect(),
    }
}

/// Replace the show's layout with the measured page. Each effect and automation
/// row moves from the show's layout node to the page node with the same name.
pub(super) fn bind_page_layout(
    project: &mut DonderProject,
    layout_id: &LayoutId,
    sequence_id: &SequenceId,
    page: &[BrowserPageNode],
) -> Result<(), JsValue> {
    let show = project
        .layout(layout_id)
        .ok_or_else(|| JsValue::from_str("The show's layout was not found."))?;
    let layout = page_layout(layout_id.clone(), page);
    let page_ids = layout
        .iter_fixtures()
        .map(|fixture| (fixture.name.clone(), fixture.id))
        .collect::<HashMap<_, _>>();
    let bound = show
        .iter_fixtures()
        .map(|fixture| {
            page_ids
                .get(&fixture.name)
                .map(|page_id| (fixture.id, *page_id))
                .ok_or_else(|| {
                    JsValue::from_str(&format!(
                        "The show's layout node {} is not on the page.",
                        fixture.name.as_str()
                    ))
                })
        })
        .collect::<Result<HashMap<_, _>, JsValue>>()?;
    let mut sequence = sequence(project, sequence_id)?;
    for effect in &mut sequence.effects {
        let page_id = bound[&effect.target.fixture];
        Arc::make_mut(effect).target.fixture = page_id;
    }
    for clip in &mut sequence.automation_clips {
        clip.row_target.fixture = bound[&clip.row_target.fixture];
    }
    replace(project, layout_id, sequence_id, layout, sequence)
}

fn sequence(
    project: &DonderProject,
    sequence_id: &SequenceId,
) -> Result<donder_model::Sequence, JsValue> {
    project
        .sequence(sequence_id)
        .cloned()
        .ok_or_else(|| JsValue::from_str("The show's sequence was not found."))
}

/// Replace the page layout. Effects and automation rows whose target left the
/// page are deleted in the same checked edit.
pub(super) fn apply_page_layout(
    project: &mut DonderProject,
    layout_id: &LayoutId,
    sequence_id: &SequenceId,
    page: &[BrowserPageNode],
) -> Result<(), JsValue> {
    let layout = page_layout(layout_id.clone(), page);
    let present = layout
        .iter_fixtures()
        .map(|fixture| fixture.id)
        .collect::<HashSet<_>>();
    let mut sequence = sequence(project, sequence_id)?;
    let removed = sequence
        .effects
        .iter()
        .filter(|effect| !present.contains(&effect.target.fixture))
        .map(|effect| effect.id.clone())
        .collect::<HashSet<_>>();
    sequence
        .effects
        .retain(|effect| !removed.contains(&effect.id));
    sequence
        .automation_clips
        .retain(|clip| present.contains(&clip.row_target.fixture));
    for clip in &mut sequence.automation_clips {
        clip.remove_bindings(|target| {
            matches!(target, AutomationTarget::EffectParam { effect_id, .. } if removed.contains(effect_id))
        });
    }
    replace(project, layout_id, sequence_id, layout, sequence)
}

fn replace(
    project: &mut DonderProject,
    layout_id: &LayoutId,
    sequence_id: &SequenceId,
    layout: Layout,
    sequence: donder_model::Sequence,
) -> Result<(), JsValue> {
    project
        .apply_edits([
            ProjectEdit::ReplaceLayout {
                id: layout_id.clone(),
                value: layout,
            },
            ProjectEdit::ReplaceSequence {
                id: sequence_id.clone(),
                value: sequence,
            },
        ])
        .map_err(|error| JsValue::from_str(&error))
}
