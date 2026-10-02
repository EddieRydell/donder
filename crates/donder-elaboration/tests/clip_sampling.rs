use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare};
use donder_language::layout::{FixtureInstanceId, LayoutFixture, LayoutFixtureKind};
use donder_language::sequence::{
    AutomationTarget, CompositionGraphNodeKind, EffectGraphEdge, GraphPortId,
};
use donder_runtime::{LoadLimits, SampleTime, decode_sequence, encode_sequence};

fn target_fixtures(nodes: &[LayoutFixture], target: FixtureInstanceId) -> Vec<u32> {
    fn visit(
        nodes: &[LayoutFixture],
        target: FixtureInstanceId,
        selected: bool,
        ids: &mut Vec<u32>,
    ) {
        for node in nodes {
            let selected = selected || node.id == target;
            match &node.kind {
                LayoutFixtureKind::Fixture { .. } if selected => ids.push(node.id.0),
                LayoutFixtureKind::Group { children } => visit(children, target, selected, ids),
                LayoutFixtureKind::Fixture { .. } => {}
            }
        }
    }
    let mut ids = Vec::new();
    visit(nodes, target, false, &mut ids);
    ids
}

#[test]
fn sparse_clips_match_full_domain_samples_and_survive_wire_roundtrips() {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let project = donder_project_io::load_project(&root).unwrap().project;
    let limits = LoadLimits {
        payload_bytes: 32 * 1024 * 1024,
        workspace_bytes: 32 * 1024 * 1024,
        ..LoadLimits::default()
    };
    let mut sample_checked = false;
    for sequence in project
        .root()
        .sequences
        .iter()
        .map(|source| project.sequence(source.id()).unwrap())
    {
        let prepared = prepare(&project, &sequence.id, PrepareOutputs::All).unwrap();
        let decoded = decode_sequence(&encode_sequence(&prepared).unwrap(), limits).unwrap();
        for authored in sequence.effects.iter().take(2) {
            sample_checked = true;
            // Prepare the authored clip alone through a plain layer/output graph.
            // The reference uses full playback; the decoded sequence uses sparse sampling.
            let mut reference_project = project.clone();
            let mut reference = sequence.clone();
            reference.effects = vec![authored.clone()];
            reference
                .layers
                .retain(|layer| layer.id == authored.layer_id);
            reference.layers[0].enabled = true;
            reference
                .composition_graph
                .nodes
                .retain(|node| match &node.kind {
                    CompositionGraphNodeKind::Layer { layer_id } => *layer_id == authored.layer_id,
                    CompositionGraphNodeKind::Output => true,
                    CompositionGraphNodeKind::Operator(_) => false,
                });
            let layer = reference
                .composition_graph
                .nodes
                .iter()
                .find(|node| matches!(node.kind, CompositionGraphNodeKind::Layer { .. }))
                .unwrap()
                .id
                .clone();
            let output = reference
                .composition_graph
                .nodes
                .iter()
                .find(|node| matches!(node.kind, CompositionGraphNodeKind::Output))
                .unwrap()
                .id
                .clone();
            reference.composition_graph.edges = vec![EffectGraphEdge {
                from: layer,
                from_port: GraphPortId("output".into()),
                to: output,
                to_port: GraphPortId("input".into()),
            }];
            for automation in &mut reference.automation_clips {
                automation.bindings.retain(|binding| {
                    matches!(&binding.target,
                        AutomationTarget::EffectParam { effect_id, .. } if *effect_id == authored.id
                    )
                });
                automation.detached_bindings.clear();
            }
            reference
                .automation_clips
                .retain(|clip| !clip.bindings.is_empty());
            reference_project
                .replace_sequence(&sequence.id, reference)
                .unwrap();
            let mut workspace = prepare(&reference_project, &sequence.id, PrepareOutputs::All)
                .unwrap()
                .into_playback();
            let fixtures = target_fixtures(
                &project.layout(&authored.target.layout).unwrap().fixtures,
                authored.target.fixture,
            );
            let clip = decoded.clip(authored.id.0).unwrap();
            let target_count = clip.target_pixel_count();
            for row_limit in [1, 17, target_count] {
                let rows = row_limit.min(target_count);
                let mut sampler = clip.sampler(rows);
                for fraction in [0.75, 0.0, 0.25, 0.75] {
                    let time = SampleTime::from_ticks(
                        clip.start_time().as_ticks()
                            + (clip.duration().as_ticks() as f64 * fraction) as u32,
                    );
                    let expected = workspace
                        .evaluate(time)
                        .fixtures()
                        .filter(|fixture| fixtures.contains(&fixture.fixture_id))
                        .flat_map(|fixture| fixture.pixels.iter().copied())
                        .collect::<Vec<_>>();
                    assert_eq!(expected.len(), target_count);
                    let actual = sampler.evaluate(time);
                    for (row, color) in actual.iter().enumerate() {
                        let index = if rows == 1 {
                            0
                        } else {
                            (row * (target_count - 1) + (rows - 1) / 2) / (rows - 1)
                        };
                        assert_eq!(
                            *color, expected[index],
                            "clip={} row={row} time={time:?}",
                            authored.id.0
                        );
                    }
                }
            }
        }
        assert!(prepared.clip(u32::MAX).is_none());
    }
    assert!(sample_checked);
}
