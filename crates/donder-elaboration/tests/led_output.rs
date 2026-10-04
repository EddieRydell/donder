use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare};
use donder_language::values::sample_time_from_frame;

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
    let mut workspace = output.clone().into_playback();
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
