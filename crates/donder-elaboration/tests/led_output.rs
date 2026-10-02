use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, PreparedSequence, prepare};
use donder_language::values::sample_time_from_frame;
use donder_runtime::wire::{LoadError, LoadLimits, decode_sequence, encode_sequence};

fn project() -> donder_project_io::ProjectSession {
    donder_project_io::load_project(
        &Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter"),
    )
    .unwrap()
}

#[test]
fn authored_led_routes_reject_overlap_bad_ranges_and_invalid_channel_order() {
    use donder_language::patch::{PixelEncoding, PixelRouteId, PixelSpan};
    use donder_language::validation::validate_project;
    let mut project = project().project;
    let patch_id = project.setups[project.root.setup.id()].patch.id().clone();
    let patch = project.patches.get_mut(&patch_id).unwrap();
    patch.routes.truncate(1);
    patch.routes[0].pixels = Some(PixelSpan { start: 0, count: 1 });
    let mut second = patch.routes[0].clone();
    second.id = PixelRouteId(1000);
    second.start_slot = 2;
    patch.routes.push(second);
    assert!(
        validate_project(&project)
            .unwrap_err()
            .to_string()
            .contains("overlap")
    );
    project.patches.get_mut(&patch_id).unwrap().routes[1].start_slot = 3;
    validate_project(&project).unwrap();
    project.patches.get_mut(&patch_id).unwrap().routes[1].pixels = Some(PixelSpan {
        start: 113,
        count: 1,
    });
    assert!(validate_project(&project).is_err());
    project.patches.get_mut(&patch_id).unwrap().routes[1].pixels =
        Some(PixelSpan { start: 0, count: 0 });
    assert!(validate_project(&project).is_err());
    project.patches.get_mut(&patch_id).unwrap().routes[1].pixels =
        Some(PixelSpan { start: 0, count: 1 });
    project.patches.get_mut(&patch_id).unwrap().routes[1].encoding =
        PixelEncoding::Rgb { order: [0, 0, 2] };
    assert!(validate_project(&project).is_err());
}

#[test]
fn starter_frame_checksums_survive_fixture_lowering_and_direct_led_packing() {
    let session = project();
    let project = &session.project;
    let sequence = project
        .sequences
        .keys()
        .find(|id| id.0.root_source().object() == "layer_test")
        .unwrap();
    let output = prepare(project, sequence, PrepareOutputs::All).unwrap();
    let signal = output.signals();
    let mut workspace = output.workspace().unwrap();
    let mut buffers: Vec<_> = output
        .outputs()
        .iter()
        .map(|port| vec![0; port.width as usize])
        .collect();
    for (frame, expected) in [
        (8398, 0x8bb5_7d05_87a6_9ae8),
        (8450, 0x5bee_7460_eba9_0468),
        (8494, 0xadc5_9683_e46e_175f),
    ] {
        let rendered = signal.evaluate_frame(frame).unwrap();
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut feed = |byte: u8| {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        };
        for byte in u64::from(frame).to_le_bytes() {
            feed(byte);
        }
        for fixture in &rendered.fixtures {
            for byte in fixture.fixture_id.to_le_bytes() {
                feed(byte);
            }
            for color in &fixture.pixels {
                for byte in [color.red, color.green, color.blue] {
                    feed(byte);
                }
            }
        }
        assert_eq!(hash, expected, "frame {frame}");
        output
            .evaluate(
                sample_time_from_frame(frame, signal.frame_rate).unwrap(),
                &mut buffers,
                &mut workspace,
            )
            .unwrap();
        for (port, buffer) in output.outputs().iter().zip(&buffers) {
            let fixture = rendered
                .fixtures
                .iter()
                .find(|fixture| fixture.fixture_id == port.port)
                .unwrap();
            let expected: Vec<_> = fixture
                .pixels
                .iter()
                .flat_map(|color| [color.green, color.red, color.blue])
                .collect();
            assert_eq!(*buffer, expected);
        }
    }
}

#[test]
fn selected_ports_and_portable_archive_preserve_full_project_pixel_coordinates() {
    let session = project();
    let project = &session.project;
    let sequence = project
        .sequences
        .keys()
        .find(|id| id.0.root_source().object() == "layer_test")
        .unwrap();
    let full = prepare(project, sequence, PrepareOutputs::All).unwrap();
    let setup = &project.setups[project.root.setup.id()];
    let ports: Vec<_> = [2, 17]
        .into_iter()
        .map(|index| {
            let output = &full.outputs()[index];
            (
                setup.controllers[output.controller_index].id().clone(),
                donder_language::controller::ControllerPortId(output.port),
            )
        })
        .collect();
    let selected = prepare(project, sequence, PrepareOutputs::Ports(&ports)).unwrap();
    let limits = LoadLimits {
        payload_bytes: 32 * 1024 * 1024,
        workspace_bytes: 32 * 1024 * 1024,
        ..LoadLimits::default()
    };
    let decoded = decode_sequence(&encode_sequence(&selected).unwrap(), limits).unwrap();
    let mut workspace = decoded.workspace().unwrap();
    let mut full_workspace = full.workspace().unwrap();
    let mut buffers: Vec<_> = decoded
        .outputs()
        .iter()
        .map(|port| vec![0; port.width as usize])
        .collect();
    let mut full_buffers: Vec<_> = full
        .outputs()
        .iter()
        .map(|port| vec![0; port.width as usize])
        .collect();
    for frame_index in [8398, 8450, 8494] {
        let time = sample_time_from_frame(frame_index, full.signals().frame_rate).unwrap();
        decoded
            .evaluate(time, &mut buffers, &mut workspace)
            .unwrap();
        full.evaluate(time, &mut full_buffers, &mut full_workspace)
            .unwrap();
        for (buffer, index) in buffers.iter().zip([2, 17]) {
            assert_eq!(buffer, &full_buffers[index]);
        }
    }
    let mut invalid_patch = selected.patch().clone();
    invalid_patch.routes[0].encoding =
        donder_runtime::patch::PixelEncoding::Rgb { order: [0, 1, 4] };
    let invalid = PreparedSequence::new(
        selected.signals().clone(),
        invalid_patch,
        selected.outputs().into(),
    );
    assert!(matches!(
        decode_sequence(&encode_sequence(&invalid).unwrap(), limits),
        Err(LoadError::InvalidSequence)
    ));
}
