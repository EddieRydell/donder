use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare};
use donder_language::values::sample_time_from_frame;
use donder_runtime::{LoadLimits, decode_sequence, encode_sequence};

fn project() -> donder_project_io::ProjectSession {
    donder_project_io::load_project(
        &Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter"),
    )
    .unwrap()
}

#[test]
fn authored_led_routes_reject_overlap_bad_ranges_and_invalid_channel_order() {
    use donder_language::patch::{PixelEncoding, PixelRouteId, PixelSpan};
    let mut project = project().project;
    let patch_id = project.reusable_setups()[project.root().setup.id()]
        .patch
        .id()
        .clone();
    let mut patch = project.patch(&patch_id).unwrap().clone();
    patch.routes.truncate(1);
    patch.routes[0].pixels = Some(PixelSpan { start: 0, count: 1 });
    let mut second = patch.routes[0].clone();
    second.id = PixelRouteId(1000);
    second.start_slot = 2;
    patch.routes.push(second);
    assert!(
        project
            .replace_patch(&patch_id, patch.clone())
            .unwrap_err()
            .to_string()
            .contains("overlap")
    );
    patch.routes[1].start_slot = 3;
    project.replace_patch(&patch_id, patch.clone()).unwrap();
    patch.routes[1].pixels = Some(PixelSpan {
        start: 113,
        count: 1,
    });
    assert!(project.replace_patch(&patch_id, patch.clone()).is_err());
    patch.routes[1].pixels = Some(PixelSpan { start: 0, count: 0 });
    assert!(project.replace_patch(&patch_id, patch.clone()).is_err());
    patch.routes[1].pixels = Some(PixelSpan { start: 0, count: 1 });
    patch.routes[1].encoding = PixelEncoding::Rgb { order: [0, 0, 2] };
    assert!(project.replace_patch(&patch_id, patch).is_err());
}

#[test]
fn starter_frame_checksums_survive_fixture_lowering_and_direct_led_packing() {
    let session = project();
    let project = &session.project;
    let sequence = project
        .reusable_sequences()
        .keys()
        .find(|id| id.0.root_source().object() == "layer_test")
        .unwrap();
    let output = prepare(project, sequence, PrepareOutputs::All).unwrap();
    let mut workspace = prepare(project, sequence, PrepareOutputs::All)
        .unwrap()
        .into_playback();
    let mut buffers: Vec<_> = output
        .outputs()
        .iter()
        .map(|port| vec![0; port.width])
        .collect();
    for (frame, expected) in [
        (8398, 0x8bb5_7d05_87a6_9ae8),
        (8450, 0x5bee_7460_eba9_0468),
        (8494, 0xadc5_9683_e46e_175f),
    ] {
        let rendered =
            workspace.evaluate(sample_time_from_frame(frame, output.frame_rate()).unwrap());
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut feed = |byte: u8| {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        };
        for byte in u64::from(frame).to_le_bytes() {
            feed(byte);
        }
        for fixture in rendered.fixtures() {
            for byte in fixture.fixture_id.to_le_bytes() {
                feed(byte);
            }
            for color in fixture.pixels {
                for byte in [color.red, color.green, color.blue] {
                    feed(byte);
                }
            }
        }
        assert_eq!(hash, expected, "frame {frame}");
        for (buffer, port) in buffers.iter_mut().zip(rendered.outputs()) {
            buffer.copy_from_slice(port.bytes);
        }
        for (port, buffer) in output.outputs().iter().zip(&buffers) {
            let fixture = rendered
                .fixtures()
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
        .reusable_sequences()
        .keys()
        .find(|id| id.0.root_source().object() == "layer_test")
        .unwrap();
    let full = prepare(project, sequence, PrepareOutputs::All).unwrap();
    let setup = &project.reusable_setups()[project.root().setup.id()];
    let ports: Vec<_> = [2, 17]
        .into_iter()
        .map(|index| {
            let output = &full.outputs()[index];
            (
                setup.controllers[output.controller_index as usize]
                    .id()
                    .clone(),
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
    let mut workspace = decoded.into_playback();
    let mut full_workspace = prepare(project, sequence, PrepareOutputs::All)
        .unwrap()
        .into_playback();
    let mut buffers: Vec<_> = workspace
        .sequence()
        .outputs()
        .iter()
        .map(|port| vec![0; port.width])
        .collect();
    let mut full_buffers: Vec<_> = full
        .outputs()
        .iter()
        .map(|port| vec![0; port.width])
        .collect();
    for frame_index in [8398, 8450, 8494] {
        let time = sample_time_from_frame(frame_index, full.frame_rate()).unwrap();
        for (snapshot, output) in buffers.iter_mut().zip(workspace.evaluate(time).outputs()) {
            snapshot.copy_from_slice(output.bytes);
        }
        for (snapshot, output) in full_buffers
            .iter_mut()
            .zip(full_workspace.evaluate(time).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        for (buffer, index) in buffers.iter().zip([2, 17]) {
            assert_eq!(buffer, &full_buffers[index]);
        }
    }
}
