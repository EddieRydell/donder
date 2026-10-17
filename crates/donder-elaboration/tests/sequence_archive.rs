use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare};
use donder_runtime::LoadError;
use donder_runtime::LoadLimits;
use donder_runtime::header_bytes;
use donder_runtime_types::sample_time_from_frame;

#[test]
fn selected_sequences_roundtrip_and_structurally_corrupt_uploads_are_rejected() {
    let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let project = donder_project_io::load_project(&path).unwrap().project;
    let setup = &project.reusable_setups()[project.root().setup.id()];
    let controller = setup.controllers[0].id();
    let port = project.reusable_controllers()[controller].ports[0].id;
    for id in project.root().sequences.iter().map(|source| source.id()) {
        let prepared = prepare(
            &project,
            id,
            PrepareOutputs::Ports(&[(controller.clone(), port)]),
        )
        .unwrap();
        let original = prepared;
        let bytes = original.encode();
        let decoded =
            donder_runtime::PreparedSequence::decode(&bytes, LoadLimits::default()).unwrap();
        assert_eq!(
            decoded.encode(),
            bytes,
            "sharing or data changed during roundtrip"
        );
        let frame_rate = original.frame_rate();
        let mut original_workspace = original.into_playback();
        let mut decoded_workspace = decoded.into_playback();
        let mut expected = original_workspace
            .sequence()
            .outputs()
            .iter()
            .map(|port| vec![0; port.width as usize])
            .collect::<Vec<_>>();
        let mut actual = expected.clone();
        for frame in [9504, 7150, 8450, 0, 8494, 8398, 15000] {
            let time = sample_time_from_frame(frame, frame_rate).unwrap();
            for (snapshot, output) in expected
                .iter_mut()
                .zip(original_workspace.evaluate(time).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
            for (snapshot, output) in actual
                .iter_mut()
                .zip(decoded_workspace.evaluate(time).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
            assert_eq!(actual, expected);
        }
        for end in [0, header_bytes() - 1, header_bytes(), bytes.len() - 1] {
            assert!(
                donder_runtime::PreparedSequence::decode(&bytes[..end], LoadLimits::default())
                    .is_err()
            );
        }
        let mut corrupt = bytes.clone();
        corrupt[header_bytes()] ^= 0x80;
        assert!(matches!(
            donder_runtime::PreparedSequence::decode(&corrupt, LoadLimits::default()),
            Err(LoadError::Checksum)
        ));
        corrupt[header_bytes()..].fill(0xff);
        // The checksum covers the section table and payload after the header.
        let fixed = size_of::<donder_runtime_types::image::Header>();
        let checksum = crc32fast::hash(&corrupt[fixed..]);
        corrupt[12..16].copy_from_slice(&checksum.to_le_bytes());
        assert!(matches!(
            donder_runtime::PreparedSequence::decode(&corrupt, LoadLimits::default()),
            Err(LoadError::Archive)
        ));
        let mut version = bytes.clone();
        version[4..8].copy_from_slice(&999u32.to_le_bytes());
        assert!(matches!(
            donder_runtime::PreparedSequence::decode(&version, LoadLimits::default()),
            Err(LoadError::Version)
        ));
        assert!(matches!(
            donder_runtime::PreparedSequence::decode(
                &bytes,
                LoadLimits {
                    workspace_bytes: 0,
                    ..LoadLimits::default()
                }
            ),
            Err(LoadError::Limit)
        ));
        assert!(matches!(
            donder_runtime::PreparedSequence::decode(
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
}
