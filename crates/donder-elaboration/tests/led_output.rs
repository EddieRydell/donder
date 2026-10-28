use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare};
use donder_runtime_types::sample_time_from_frame;

fn project() -> donder_project_io::ProjectSession {
    donder_project_io::check_project(
        &Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter"),
        &Default::default(),
        &mut Default::default(),
    )
    .into_session()
    .unwrap()
}

#[test]
fn authored_led_routes_reject_overlap_bad_ranges_and_invalid_channel_order() {
    use donder_model::{PixelRouteId, PixelSpan};
    use donder_runtime_types::PixelEncoding;
    let mut project = project().project;
    let patch_id = *project.reusable::<donder_model::Setup>()[project.root().setup.id()]
        .patch
        .id();
    let mut patch = project
        .get::<donder_model::Patch>(patch_id)
        .unwrap()
        .clone();
    patch.routes.truncate(1);
    patch.routes[0].pixels = Some(PixelSpan { start: 0, count: 1 });
    let mut second = patch.routes[0].clone();
    second.id = PixelRouteId(1000);
    second.start_slot = 2;
    patch.routes.push(second);
    assert!(matches!(
        project.put(patch.clone()).unwrap_err(),
        donder_model::EditError::Invalid(donder_model::ProjectValidationError {
            problem: donder_model::ValidationProblem::Route {
                problem: donder_model::RouteProblem::OutputsOverlap,
                ..
            },
            ..
        })
    ));
    patch.routes[1].start_slot = 3;
    project.put(patch.clone()).unwrap();
    patch.routes[1].pixels = Some(PixelSpan {
        start: 113,
        count: 1,
    });
    assert!(project.put(patch.clone()).is_err());
    patch.routes[1].pixels = Some(PixelSpan { start: 0, count: 0 });
    assert!(project.put(patch.clone()).is_err());
    patch.routes[1].pixels = Some(PixelSpan { start: 0, count: 1 });
    patch.routes[1].encoding = PixelEncoding::Rgb { order: [0, 0, 2] };
    assert!(project.put(patch).is_err());
}

#[test]
fn starter_frame_checksums_survive_fixture_lowering_and_direct_led_packing() {
    let session = project();
    let project = &session.project;
    let sequence = project
        .reusable::<donder_model::Sequence>()
        .keys()
        .find(|id| session.declaration((**id).into()).unwrap().1.as_str() == "layer_test")
        .unwrap();
    let output = prepare(project, sequence, PrepareOutputs::All).unwrap();
    let mut workspace = output.clone().into_playback();
    let mut buffers: Vec<_> = output
        .outputs()
        .iter()
        .map(|port| vec![0; port.width as usize])
        .collect();
    let mut hashes = Vec::new();
    for frame in [8398, 8450, 8494] {
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
                for byte in color.to_bytes() {
                    feed(byte);
                }
            }
        }
        hashes.push(hash);
        for (buffer, port) in buffers.iter_mut().zip(rendered.outputs()) {
            buffer.copy_from_slice(port.bytes);
        }
        // The starter routes its fixtures to ports in order, one each.
        for (fixture, buffer) in rendered.fixtures().zip(&buffers) {
            let expected: Vec<_> = fixture
                .pixels
                .iter()
                .flat_map(|color| {
                    let [red, green, blue] = color.to_bytes();
                    [green, red, blue]
                })
                .collect();
            assert_eq!(*buffer, expected);
        }
    }
    assert_eq!(
        hashes,
        [
            0xa86e_2634_3f3b_69f3,
            0xbc0c_32fc_34f1_6b8a,
            0x1eb6_eec0_ec80_1ba7
        ]
    );
}
