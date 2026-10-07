use super::*;
use donder_language::sequence::{AutomationDetachmentReason, AutomationTarget};
use donder_sequence_api::BrowserPageNode;
use std::collections::HashSet;

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
    Layout {
        description: None,
        id,
        fixtures: page.iter().map(layout_fixture).collect(),
    }
}

fn layout_fixture(node: &BrowserPageNode) -> LayoutFixture {
    match node {
        BrowserPageNode::Group { id, name, children } => LayoutFixture {
            description: None,
            id: FixtureInstanceId(*id),
            name: donder_language::names::object_name(name),
            kind: LayoutFixtureKind::Group {
                children: children.iter().map(layout_fixture).collect(),
            },
        },
        BrowserPageNode::Fixture { id, name, pixels } => LayoutFixture {
            description: None,
            id: FixtureInstanceId(*id),
            name: donder_language::names::object_name(name),
            kind: LayoutFixtureKind::Fixture {
                definition: ValueSource::Inline(fixture_definition(pixels)),
                transform: FixtureTransform::default(),
            },
        },
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
                name: donder_language::names::object_name(&format!("pixel_{}", index + 1)),
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
    let mut sequence = project
        .sequence(sequence_id)
        .cloned()
        .ok_or_else(|| JsValue::from_str("The demo sequence was not found."))?;
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
        clip.detach_bindings(AutomationDetachmentReason::TargetDeleted, |target| {
            matches!(target, AutomationTarget::EffectParam { effect_id, .. } if removed.contains(effect_id))
        });
    }
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
