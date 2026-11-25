use camino::Utf8PathBuf;
use donder_elaboration::{Retain, prepare};
use donder_model::{AutomationTarget, CompositionGraphNodeKind, EffectGraphEdge};
use donder_runtime_types::SampleTime;

#[test]
fn sparse_clips_match_full_domain_samples_and_survive_archive_roundtrips() {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let project =
        donder_project_io::check_project(&root, &Default::default(), &mut Default::default())
            .into_session()
            .unwrap()
            .project()
            .clone();
    let mut sample_checked = false;
    for (sequence_id, sequence) in project.objects::<donder_model::Sequence>() {
        let prepared = prepare(&project, &sequence_id, Retain::All).unwrap();
        let decoded = donder_runtime::PreparedShow::decode(&prepared.encode()).unwrap();
        for authored in sequence
            .composition
            .clips
            .iter()
            .filter(|clip| clip.effect().is_some())
            .take(2)
        {
            let layer = &authored.effect().unwrap().layer;
            sample_checked = true;
            // Prepare the authored clip alone through a plain layer/output graph.
            // The reference uses full playback; the decoded sequence uses sparse sampling.
            let mut reference_project = project.clone();
            let mut reference = sequence.clone();
            // The authored clip and the envelopes that drive it.
            reference.composition.clips = sequence
                .composition
                .clips
                .iter()
                .filter(|clip| clip.envelope().is_some() || clip.id == authored.id)
                .cloned()
                .collect();
            reference
                .composition
                .layers
                .retain(|candidate| candidate.id == *layer);
            reference.composition.layers[0].enabled = true;
            reference
                .composition
                .graph
                .nodes
                .retain(|node| match &node.kind {
                    CompositionGraphNodeKind::Layer { layer_id } => layer_id == layer,
                    CompositionGraphNodeKind::Root => true,
                    CompositionGraphNodeKind::Operator(_) => false,
                });
            let layer = reference
                .composition
                .graph
                .nodes
                .iter()
                .find(|node| matches!(node.kind, CompositionGraphNodeKind::Layer { .. }))
                .unwrap()
                .id
                .clone();
            let output = reference
                .composition
                .graph
                .nodes
                .iter()
                .find(|node| matches!(node.kind, CompositionGraphNodeKind::Root))
                .unwrap()
                .id
                .clone();
            reference.composition.graph.edges = vec![EffectGraphEdge {
                from: layer,
                to: output,
                input: None,
            }];
            for clip in &mut reference.composition.clips {
                if let Some(envelope) = std::sync::Arc::make_mut(clip).envelope_mut() {
                    envelope.bindings.retain(|binding| {
                        matches!(&binding.target,
                            AutomationTarget::ClipParam { clip_id, .. } if *clip_id == authored.id
                        )
                    });
                }
            }
            reference.composition.clips.retain(|clip| {
                clip.envelope()
                    .is_none_or(|envelope| !envelope.bindings.is_empty())
            });
            reference_project.put((sequence_id, reference)).unwrap();
            let mut workspace = prepare(&reference_project, &sequence_id, Retain::All)
                .unwrap()
                .into_playback();
            let fixtures = project
                .layout()
                .members(authored.target)
                .into_iter()
                .map(|part| part.fixture.0)
                .collect::<Vec<_>>();
            let clip = decoded.clip(authored.id.0).unwrap();
            let target_count = clip.target_pixel_count();
            for row_limit in [1, 17, target_count] {
                let rows = row_limit.min(target_count);
                let mut raster = clip.raster(rows);
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
                    let actual = raster.evaluate(time);
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
