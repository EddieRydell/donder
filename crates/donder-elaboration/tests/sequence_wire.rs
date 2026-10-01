use camino::Utf8PathBuf;
use donder_elaboration::PreparedSequenceOutput;
use donder_runtime::dsl::bytecode::{ColorSlot, Instruction, ValueSlot};
use donder_runtime::values::sample_time_from_frame;
use donder_runtime::wire::{
    HEADER_BYTES, LoadError, LoadLimits, decode_sequence, encode_sequence,
    validate_prepared_sequence, validate_prepared_signal_graph,
};

#[test]
fn selected_sequences_roundtrip_and_corrupt_uploads_are_rejected() {
    let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let project = donder_project_io::load_project(&path).unwrap().project;
    let setup = &project.setups[project.root.setup.id()];
    let controller = setup.controllers[0].id();
    let port = project.controllers[controller].ports[0].id;
    let mut tested_invalid_bytecode = false;
    let mut tested_invalid_frame_plan = false;
    for id in project.root.sequences.iter().map(|source| source.id()) {
        let prepared = PreparedSequenceOutput::prepare_selected(
            &project,
            project.root.setup.id(),
            id,
            &[(controller.clone(), port)],
        )
        .unwrap();
        let original = prepared.sequence;
        let bytes = encode_sequence(&original).unwrap();
        let decoded = decode_sequence(&bytes, LoadLimits::default()).unwrap();
        let mut invalid_bytecode = decode_sequence(&bytes, LoadLimits::default()).unwrap();
        if let Some(first_program) = invalid_bytecode.signals.programs.first_mut() {
            first_program.instructions[0] = Instruction::ReturnColor(ColorSlot(u32::MAX));
            assert!(matches!(
                decode_sequence(
                    &encode_sequence(&invalid_bytecode).unwrap(),
                    LoadLimits::default()
                ),
                Err(LoadError::InvalidSequence)
            ));
            tested_invalid_bytecode = true;
        }
        let mut wrong_return = decode_sequence(&bytes, LoadLimits::default()).unwrap();
        if let Some(first_program) = wrong_return.signals.programs.first_mut() {
            let instruction = first_program
                .instructions
                .iter_mut()
                .find(|instruction| matches!(instruction, Instruction::ReturnColor(_)))
                .expect("sample program returns color");
            let Instruction::ReturnColor(slot) = instruction else {
                unreachable!()
            };
            *instruction = Instruction::Return(ValueSlot::Color(*slot));
            assert!(matches!(
                decode_sequence(
                    &encode_sequence(&wrong_return).unwrap(),
                    LoadLimits::default()
                ),
                Err(LoadError::InvalidSequence)
            ));
        }
        let mut missing_frame_input = decode_sequence(&bytes, LoadLimits::default()).unwrap();
        let output = missing_frame_input.signals.plan.output_index;
        let input = match &missing_frame_input.signals.plan.nodes[output].kind {
            donder_runtime::signal::PreparedSignalKind::Output { inputs } => {
                inputs.first().copied()
            }
            _ => None,
        };
        if let Some(input) = input {
            missing_frame_input.signals.plan.frame_nodes = missing_frame_input
                .signals
                .plan
                .frame_nodes
                .iter()
                .copied()
                .filter(|&node| node != input)
                .collect();
            assert!(matches!(
                decode_sequence(
                    &encode_sequence(&missing_frame_input).unwrap(),
                    LoadLimits::default()
                ),
                Err(LoadError::InvalidSequence)
            ));
            tested_invalid_frame_plan = true;
        }
        assert_eq!(
            encode_sequence(&decoded).unwrap(),
            bytes,
            "sharing or data changed during roundtrip"
        );
        let mut original_workspace = original.workspace().unwrap();
        let mut decoded_workspace = decoded.workspace().unwrap();
        let mut expected = original
            .output_widths
            .iter()
            .map(|&width| vec![0; width as usize])
            .collect::<Vec<_>>();
        let mut actual = expected.clone();
        for frame in [9504, 7150, 8450, 0, 8494, 8398, 15000] {
            let time = sample_time_from_frame(frame, original.signals.frame_rate).unwrap();
            original
                .evaluate(time, &mut expected, &mut original_workspace)
                .unwrap();
            decoded
                .evaluate(time, &mut actual, &mut decoded_workspace)
                .unwrap();
            assert_eq!(actual, expected);
        }
        let mut playback = decode_sequence(&bytes, LoadLimits::default())
            .unwrap()
            .into_playback()
            .unwrap();
        let time = sample_time_from_frame(8398, original.signals.frame_rate).unwrap();
        let mut owned = actual.clone();
        original
            .evaluate(time, &mut expected, &mut original_workspace)
            .unwrap();
        playback.evaluate(time, &mut owned).unwrap();
        assert_eq!(owned, expected);
        assert_eq!(
            playback.rendered_fixtures().unwrap(),
            original.rendered_fixtures(&original_workspace).unwrap()
        );
        for end in [0, HEADER_BYTES - 1, HEADER_BYTES, bytes.len() - 1] {
            assert!(decode_sequence(&bytes[..end], LoadLimits::default()).is_err());
        }
        let mut corrupt = bytes.clone();
        corrupt[HEADER_BYTES] ^= 0x80;
        assert!(matches!(
            decode_sequence(&corrupt, LoadLimits::default()),
            Err(LoadError::Checksum)
        ));
        corrupt[16..].fill(0xff);
        let checksum = crc32fast::hash(&corrupt[16..]);
        corrupt[12..16].copy_from_slice(&checksum.to_le_bytes());
        assert!(matches!(
            decode_sequence(&corrupt, LoadLimits::default()),
            Err(LoadError::Archive)
        ));
        let mut version = bytes.clone();
        version[4..8].copy_from_slice(&999u32.to_le_bytes());
        assert!(matches!(
            decode_sequence(&version, LoadLimits::default()),
            Err(LoadError::Version)
        ));
        assert!(matches!(
            decode_sequence(
                &bytes,
                LoadLimits {
                    workspace_bytes: 0,
                    ..LoadLimits::default()
                }
            ),
            Err(LoadError::Limit)
        ));
        let mut invalid = original;
        let saved_count = invalid.signals.targets[0].sample_count;
        let saved_pixel = invalid.signals.target_pixels[0].clone();
        invalid.signals.targets[0].sample_count = 1;
        invalid.signals.target_pixels[0].pixel_index = 1;
        invalid.signals.target_pixels[0].pixel_count = 2;
        assert!(
            matches!(
                decode_sequence(&encode_sequence(&invalid).unwrap(), LoadLimits::default()),
                Err(LoadError::InvalidSequence)
            ),
            "an upload must not be able to index past the prepared sample cache"
        );
        invalid.signals.targets[0].sample_count = saved_count;
        invalid.signals.target_pixels[0] = saved_pixel;
        invalid.signals.plan.output_index = usize::MAX;
        let invalid_bytes = encode_sequence(&invalid).unwrap();
        assert!(matches!(
            decode_sequence(&invalid_bytes, LoadLimits::default()),
            Err(LoadError::InvalidSequence)
        ));
        assert!(matches!(
            decode_sequence(
                &bytes,
                LoadLimits {
                    pixels: 1,
                    ..LoadLimits::default()
                }
            ),
            Err(LoadError::Limit)
        ));
        println!(
            "{} archive bytes={}",
            id.0.root_source().object(),
            bytes.len()
        );
    }
    assert!(
        tested_invalid_bytecode,
        "starter must exercise bytecode validation"
    );
    assert!(
        tested_invalid_frame_plan,
        "starter must exercise frame-plan validation"
    );
}

#[test]
fn wire_rejects_reused_nested_operator_vm_slot() {
    use donder_runtime::dsl::BoundParams;
    use donder_runtime::signal::{
        PreparedOperator, PreparedOperatorNode, PreparedSignalKind, PreparedSignalNode,
    };

    let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let project = donder_project_io::load_project(&path).unwrap().project;
    let mut sequence = PreparedSequenceOutput::prepare(
        &project,
        project.root.setup.id(),
        project.root.sequences[0].id(),
    )
    .unwrap()
    .sequence;
    let identity = donder_language::dsl::compile_operators(
        "operator Identity { input Signal source; color sample() { return source.at(seconds()); } }",
    )
    .unwrap()
    .remove(0);
    let program = sequence.signals.programs.len() as u32;
    let mut programs = sequence.signals.programs.to_vec();
    programs.push(identity.bytecode);
    sequence.signals.programs = programs.into();

    let first = sequence.signals.plan.nodes.len();
    let first_slot = sequence.signals.plan.vm_workspace_count as u16;
    let second = first + 1;
    let node = |input, slot| PreparedSignalNode {
        kind: PreparedSignalKind::Operator {
            operator: PreparedOperatorNode {
                automation_slot: 0,
                implementation: PreparedOperator::Dsl(program),
                params: BoundParams::default(),
            },
            inputs: vec![input].into(),
            automation: Box::new([]),
            vm_slot: slot,
        },
    };
    let mut nodes = sequence.signals.plan.nodes.to_vec();
    nodes.push(node(sequence.signals.plan.output_index, first_slot));
    nodes.push(node(first, first_slot + 1));
    sequence.signals.plan.nodes = nodes.into();
    sequence.signals.plan.output_index = second;
    sequence.signals.plan.vm_workspace_count += 2;
    sequence.signals.plan.frame_nodes = vec![second].into();
    sequence.signals.plan.frame_slots = vec![u16::MAX; second + 1].into();
    sequence.signals.plan.frame_slots[second] = 0;
    sequence.signals.plan.frame_buffer_count = 1;
    assert!(decode_sequence(&encode_sequence(&sequence).unwrap(), LoadLimits::default()).is_ok());

    let PreparedSignalKind::Operator { vm_slot, .. } =
        &mut sequence.signals.plan.nodes[second].kind
    else {
        unreachable!()
    };
    *vm_slot = first_slot;
    assert!(matches!(
        decode_sequence(&encode_sequence(&sequence).unwrap(), LoadLimits::default()),
        Err(LoadError::InvalidSequence)
    ));
}

#[test]
fn wire_rejects_overwritten_output_buffer() {
    use donder_runtime::signal::{PreparedSignalKind, PreparedSignalNode};

    let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let project = donder_project_io::load_project(&path).unwrap().project;
    let sequence_id = project
        .root
        .sequences
        .iter()
        .map(|source| source.id())
        .find(|id| id.0.root_source().object() == "layer_test")
        .unwrap();
    let setup = &project.setups[project.root.setup.id()];
    let controller = setup.controllers[0].id();
    let port = project.controllers[controller].ports[0].id;
    let mut sequence = PreparedSequenceOutput::prepare_selected(
        &project,
        project.root.setup.id(),
        sequence_id,
        &[(controller.clone(), port)],
    )
    .unwrap()
    .sequence;
    assert_eq!(validate_prepared_signal_graph(&sequence.signals), Ok(()));
    assert_eq!(validate_prepared_sequence(&sequence), Ok(()));
    let workspace_count = sequence.signals.plan.vm_workspace_count;
    sequence.signals.plan.vm_workspace_count = usize::MAX;
    assert!(matches!(
        sequence.signals.workspace(),
        Err(LoadError::InvalidSequence)
    ));
    assert!(matches!(
        sequence.workspace(),
        Err(LoadError::InvalidSequence)
    ));
    assert_eq!(
        validate_prepared_signal_graph(&sequence.signals),
        Err(LoadError::InvalidSequence)
    );
    assert_eq!(
        validate_prepared_sequence(&sequence),
        Err(LoadError::InvalidSequence)
    );
    sequence.signals.plan.vm_workspace_count = workspace_count;
    let encoded = encode_sequence(&sequence).unwrap();
    let accepted = decode_sequence(&encoded, LoadLimits::default());
    assert!(accepted.is_ok(), "{:?}", accepted.err());

    let plan = &mut sequence.signals.plan;
    let extra = plan.nodes.len();
    let output_slot = plan.frame_slots[plan.output_index];
    let mut nodes = plan.nodes.to_vec();
    nodes.push(PreparedSignalNode {
        kind: PreparedSignalKind::Layer { layer_index: 0 },
    });
    plan.nodes = nodes.into();
    let mut slots = plan.frame_slots.to_vec();
    slots.push(output_slot);
    plan.frame_slots = slots.into();
    let mut scheduled = plan.frame_nodes.to_vec();
    scheduled.push(extra);
    plan.frame_nodes = scheduled.into();
    assert_eq!(
        validate_prepared_signal_graph(&sequence.signals),
        Err(LoadError::InvalidSequence)
    );
    assert_eq!(
        validate_prepared_sequence(&sequence),
        Err(LoadError::InvalidSequence)
    );
    assert!(matches!(
        decode_sequence(&encode_sequence(&sequence).unwrap(), LoadLimits::default()),
        Err(LoadError::InvalidSequence)
    ));
}
