use camino::Utf8PathBuf;
use dawn_elaboration::{PreparedSequenceOutput, elaborate_sequence};
use dawn_language::values::sample_time_from_frame;
use dawn_runtime::wire::{LoadError, LoadLimits, decode_sequence};

fn project() -> dawn_project_io::ProjectSession {
    dawn_project_io::load_package(
        &Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter"),
    )
    .unwrap()
    .session
}

#[test]
fn authored_led_routes_reject_overlap_bad_ranges_and_invalid_channel_order() {
    use dawn_language::patch::{PixelEncoding, PixelRouteId, PixelSpan};
    use dawn_language::validation::validate_project;
    let mut project = project().project;
    let patch_id = project.setups[&project.root.setup].patch.clone();
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
        .find(|id| id.0.object() == "layer_test")
        .unwrap();
    let signal = elaborate_sequence(project, &project.root.setup, sequence).unwrap();
    let output = PreparedSequenceOutput::prepare(project, &project.root.setup, sequence).unwrap();
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
        let packed = output.render_frame(frame).unwrap();
        for port in &packed.controller_frames {
            let fixture = rendered
                .fixtures
                .iter()
                .find(|fixture| fixture.fixture_id == port.port.0)
                .unwrap();
            let expected: Vec<_> = fixture
                .pixels
                .iter()
                .flat_map(|color| [color.green, color.red, color.blue])
                .collect();
            assert_eq!(port.slots, expected);
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
        .find(|id| id.0.object() == "layer_test")
        .unwrap();
    let full = PreparedSequenceOutput::prepare(project, &project.root.setup, sequence).unwrap();
    let frame = full.render_frame(8450).unwrap();
    let ports: Vec<_> = [2, 17]
        .into_iter()
        .map(|index| {
            (
                frame.controller_frames[index].controller.clone(),
                frame.controller_frames[index].port,
            )
        })
        .collect();
    let mut selected =
        PreparedSequenceOutput::prepare_selected(project, &project.root.setup, sequence, &ports)
            .unwrap();
    let limits = LoadLimits {
        payload_bytes: 32 * 1024 * 1024,
        workspace_bytes: 32 * 1024 * 1024,
        ..LoadLimits::default()
    };
    let decoded = decode_sequence(&selected.encode().unwrap(), limits).unwrap();
    let mut workspace = decoded.workspace();
    let mut buffers: Vec<_> = decoded
        .output_widths
        .iter()
        .map(|&width| vec![0; width as usize])
        .collect();
    for frame_index in [8398, 8450, 8494] {
        decoded
            .evaluate(
                sample_time_from_frame(frame_index, full.frame_rate()).unwrap(),
                &mut buffers,
                &mut workspace,
            )
            .unwrap();
        let full_frame = full.render_frame(frame_index).unwrap();
        for (buffer, (controller, port)) in buffers.iter().zip(&ports) {
            assert_eq!(
                buffer,
                &full_frame
                    .controller_frames
                    .iter()
                    .find(|frame| frame.controller == *controller && frame.port == *port)
                    .unwrap()
                    .slots
            );
        }
    }
    selected.sequence.patch.routes[0].encoding =
        dawn_runtime::patch::PixelEncoding::Rgb { order: [0, 1, 4] };
    assert!(matches!(
        decode_sequence(&selected.encode().unwrap(), limits),
        Err(LoadError::InvalidSequence)
    ));
}
