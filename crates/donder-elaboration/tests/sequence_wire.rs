use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare};
use donder_runtime::sample_time_from_frame;
use donder_runtime::{HEADER_BYTES, LoadError, LoadLimits, decode_sequence, encode_sequence};

#[test]
fn selected_sequences_roundtrip_and_corrupt_uploads_are_rejected() {
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
        let bytes = encode_sequence(&original).unwrap();
        let decoded = decode_sequence(&bytes, LoadLimits::default()).unwrap();
        assert_eq!(
            encode_sequence(&decoded).unwrap(),
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
            .map(|port| vec![0; port.width])
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
        let mut playback = decode_sequence(&bytes, LoadLimits::default())
            .unwrap()
            .into_playback();
        let time = sample_time_from_frame(8398, frame_rate).unwrap();
        let expected = original_workspace.evaluate(time);
        let actual = playback.evaluate(time);
        assert!(actual.outputs().eq(expected.outputs()));
        assert!(actual.fixtures().eq(expected.fixtures()));
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
}
