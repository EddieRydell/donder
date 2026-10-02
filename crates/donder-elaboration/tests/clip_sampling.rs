use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare};
use donder_language::dsl::EffectKind;
use donder_runtime::SampleTime;
use donder_runtime::{LoadError, LoadLimits, decode_sequence, encode_sequence};
use donder_runtime::{PreparedLayer, PreparedSignalKind, PreparedSignalNode};

#[test]
fn sparse_clips_match_full_domain_samples_and_survive_wire_roundtrips() {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let project = donder_project_io::load_project(&root).unwrap().project;
    // This exercises a full host preview, not a controller-sized upload.
    let limits = LoadLimits {
        payload_bytes: 32 * 1024 * 1024,
        workspace_bytes: 32 * 1024 * 1024,
        ..LoadLimits::default()
    };
    let mut sample_checked = false;
    let mut generator_checked = false;
    for sequence in project
        .root()
        .sequences
        .iter()
        .map(|source| project.sequence(source.id()).unwrap())
    {
        let prepared = prepare(&project, &sequence.id, PrepareOutputs::All).unwrap();
        let decoded = decode_sequence(&encode_sequence(&prepared).unwrap(), limits).unwrap();
        for kind in [EffectKind::Sample, EffectKind::Generator] {
            let Some(authored) = sequence.effects.iter().find(|effect| {
                project
                    .definitions()
                    .effects
                    .resolve(&effect.definition)
                    .unwrap()
                    .kind()
                    == kind
            }) else {
                continue;
            };
            match kind {
                EffectKind::Sample => sample_checked = true,
                EffectKind::Generator => generator_checked = true,
            }
            let descriptor = prepared
                .to_raw_signals()
                .clips
                .iter()
                .find(|clip| clip.id == authored.id.0)
                .cloned()
                .unwrap();
            let mut reference = prepared.to_raw_signals();
            let mut effects = descriptor.effects.to_vec();
            effects.sort_by_key(|&index| reference.effects[index].start_time);
            reference.layers = vec![PreparedLayer { enabled: true }].into();
            reference.effects_by_layer = vec![effects.into_boxed_slice()].into();
            reference.plan.nodes = vec![PreparedSignalNode {
                kind: PreparedSignalKind::Layer { layer_index: 0 },
            }]
            .into();
            reference.plan.output_index = 0;
            reference.plan.vm_workspace_count = 0;
            reference.plan.frame_nodes = vec![0].into();
            reference.plan.frame_slots = vec![0].into();
            reference.plan.frame_buffer_count = 1;
            let mut workspace = donder_runtime::PreparedSequence::admit(
                reference.clone(),
                donder_runtime::PreparedPatch {
                    routes: Box::new([]),
                    lookups: Box::new([]),
                },
                Box::new([]),
            )
            .unwrap()
            .into_playback();
            let clip = decoded.clip(authored.id.0).unwrap();
            let target = reference.target(descriptor.target);
            for row_limit in [1, 17, target.len()] {
                let rows = row_limit.min(target.len());
                let mut sampler = clip.sampler(rows);
                for fraction in [0.75, 0.0, 0.25, 0.75] {
                    let time = SampleTime::from_ticks(
                        descriptor.start_time.as_ticks()
                            + (descriptor.duration.as_ticks() as f64 * fraction) as u32,
                    );
                    let expected = workspace.evaluate(time).colors();
                    let actual = sampler.evaluate(time);
                    for (row, color) in actual.iter().enumerate() {
                        let index = if rows == 1 {
                            0
                        } else {
                            (row * (target.len() - 1) + (rows - 1) / 2) / (rows - 1)
                        };
                        let pixel = &target[index];
                        let address = reference.fixture_pixel_offsets[pixel.fixture_index]
                            + pixel.fixture_pixel_index as usize;
                        assert_eq!(
                            *color, expected[address],
                            "clip={} row={row} time={time:?}",
                            authored.id.0
                        );
                    }
                }
            }
        }
        assert!(prepared.clip(u32::MAX).is_none());
        let mut invalid = prepared.to_raw_signals();
        let Some(clip) = invalid.clips.first_mut() else {
            continue;
        };
        clip.target = usize::MAX;
        let invalid = donder_runtime::PreparedSequence::admit(
            invalid,
            prepared.patch().clone(),
            prepared.outputs().into(),
        );
        assert!(matches!(invalid, Err(LoadError::InvalidSequence)));
    }
    assert!(sample_checked && generator_checked);
}
