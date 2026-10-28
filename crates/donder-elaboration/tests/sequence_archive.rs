use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare};
use donder_runtime::LoadError;
use donder_runtime_types::sample_time_from_frame;

#[test]
fn selected_sequences_roundtrip_and_other_formats_are_rejected() {
    let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let session =
        donder_project_io::check_project(&path, &Default::default(), &mut Default::default())
            .into_session()
            .unwrap();
    let project = &session.project;
    let setup = &project.reusable::<donder_model::Setup>()[project.root().setup.id()];
    let controller = setup.controllers[0].id();
    let port = project.reusable::<donder_model::Controller>()[controller].ports[0].id;
    for id in project.root().sequences.iter().map(|source| source.id()) {
        let prepared = prepare(project, id, PrepareOutputs::Ports(&[(*controller, port)])).unwrap();
        let original = prepared;
        let bytes = original.encode();
        let decoded = donder_runtime::PreparedShow::decode(&bytes).unwrap();
        assert_eq!(
            decoded.encode(),
            bytes,
            "sharing or data changed during roundtrip"
        );
        let frame_rate = original.frame_rate();
        let mut original_workspace = original.into_playback();
        let mut decoded_workspace = decoded.into_playback();
        let mut expected = original_workspace
            .prepared()
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
        let mut version = bytes.clone();
        version[0..4].copy_from_slice(&999u32.to_le_bytes());
        assert!(matches!(
            donder_runtime::PreparedShow::decode(&version),
            Err(LoadError::Version)
        ));
        println!(
            "{} archive bytes={}",
            session.declaration((*id).into()).unwrap().1.as_str(),
            bytes.len()
        );
    }
}
