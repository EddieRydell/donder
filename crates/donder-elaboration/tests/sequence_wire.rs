use camino::Utf8PathBuf;
use donder_elaboration::PreparedSequenceOutput;
use donder_runtime::values::sample_time_from_frame;
use donder_runtime::wire::{HEADER_BYTES, LoadError, LoadLimits, decode_sequence, encode_sequence};

#[test]
fn selected_sequences_roundtrip_and_corrupt_uploads_are_rejected() {
    let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let project = donder_project_io::load_package(&path)
        .unwrap()
        .session
        .project;
    let setup = &project.setups[&project.root.setup];
    let controller = &setup.controllers[0];
    let port = project.controllers[controller].ports[0].id;
    for id in &project.root.sequences {
        let prepared = PreparedSequenceOutput::prepare_selected(
            &project,
            &project.root.setup,
            id,
            &[(controller.clone(), port)],
        )
        .unwrap();
        let original = prepared.sequence;
        let bytes = encode_sequence(&original).unwrap();
        let decoded = decode_sequence(&bytes, LoadLimits::default()).unwrap();
        assert_eq!(
            encode_sequence(&decoded).unwrap(),
            bytes,
            "sharing or data changed during roundtrip"
        );
        let mut original_workspace = original.workspace();
        let mut decoded_workspace = decoded.workspace();
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
        println!("{} archive bytes={}", id.0.object(), bytes.len());
    }
}
