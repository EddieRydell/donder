use camino::Utf8PathBuf;
use dawn_project_io::load_package;

use crate::PreparedSequenceOutput;

fn example(name: &str) -> dawn_project_io::ProjectSession {
    let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples")
        .join(name);
    load_package(&path)
        .map(|loaded| loaded.session)
        .unwrap_or_else(|error| panic!("failed to load {name}: {error}"))
}

#[test]
fn every_example_prepares_and_produces_exact_controller_widths() {
    for name in ["starter"] {
        let session = example(name);
        let sequence_id = session.project.root.sequences.first().unwrap();
        let renderer = PreparedSequenceOutput::prepare(
            &session.project,
            &session.project.root.setup,
            sequence_id,
        )
        .unwrap_or_else(|error| panic!("failed to prepare {name}: {error:?}"));
        let frame = renderer
            .render_seconds(0.0)
            .unwrap_or_else(|error| panic!("failed to render {name}: {error:?}"));
        let setup = session
            .project
            .setups
            .get(&session.project.root.setup)
            .unwrap();
        let expected_ports = setup
            .controllers
            .iter()
            .map(|id| session.project.controllers.get(id).unwrap().ports.len())
            .sum::<usize>();
        assert_eq!(frame.controller_frames.len(), expected_ports);
        for port_frame in &frame.controller_frames {
            let controller = session
                .project
                .controllers
                .get(&port_frame.controller)
                .unwrap();
            let port = controller
                .ports
                .iter()
                .find(|port| port.id == port_frame.port)
                .unwrap();
            assert_eq!(port_frame.slots.len(), usize::from(port.slot_count));
        }
    }
}

#[test]
fn preview_and_controller_buffers_are_from_one_deterministic_show_frame() {
    let session = example("starter");
    let sequence_id = session.project.root.sequences.first().unwrap();
    let renderer =
        PreparedSequenceOutput::prepare(&session.project, &session.project.root.setup, sequence_id)
            .unwrap();
    let first = renderer.render_frame(10).unwrap();
    let second = renderer.render_frame(10).unwrap();
    assert_eq!(first, second);
    assert!(!first.fixtures.is_empty());
    assert!(!first.controller_frames.is_empty());
    let preview_checksum = first.fixtures.iter().fold(0u64, |hash, fixture| {
        fixture.pixels.iter().copied().fold(hash, |hash, color| {
            hash.wrapping_mul(16777619)
                ^ u64::from(color.red)
                ^ (u64::from(color.green) << 8)
                ^ (u64::from(color.blue) << 16)
        })
    });
    let controller_checksum = first.controller_frames.iter().fold(0u64, |hash, frame| {
        frame.slots.iter().fold(hash, |hash, slot| {
            hash.wrapping_mul(16777619) ^ u64::from(*slot)
        })
    });
    assert_eq!(
        preview_checksum,
        second.fixtures.iter().fold(0u64, |hash, fixture| {
            fixture.pixels.iter().copied().fold(hash, |hash, color| {
                hash.wrapping_mul(16777619)
                    ^ u64::from(color.red)
                    ^ (u64::from(color.green) << 8)
                    ^ (u64::from(color.blue) << 16)
            })
        })
    );
    assert_eq!(
        controller_checksum,
        second
            .controller_frames
            .iter()
            .fold(0u64, |hash, frame| frame
                .slots
                .iter()
                .fold(hash, |hash, slot| hash.wrapping_mul(16777619)
                    ^ u64::from(*slot)))
    );
    let mut workspace = renderer.workspace();
    let sampled = renderer
        .sample_into(first.sample_time, &mut workspace)
        .unwrap();
    assert_eq!(sampled, first.controller_frames);
}

#[test]
fn starter_sequence_behavioral_checksums_run_in_the_normal_test_gate() {
    let session = example("starter");
    let sequence_id = session.project.root.sequences.get(1).unwrap();
    let renderer =
        crate::elaborate_sequence(&session.project, &session.project.root.setup, sequence_id)
            .unwrap();
    let rendered = renderer.evaluate_frame(3594).unwrap();
    assert_eq!(checksum_frame(&rendered), 0xaa28_e560_49eb_1e76);
}

#[test]
fn output_fixtures_preserve_layout_instance_order() {
    let session = example("starter");
    let sequence_id = session.project.root.sequences.first().unwrap();
    let renderer =
        PreparedSequenceOutput::prepare(&session.project, &session.project.root.setup, sequence_id)
            .unwrap();
    let frame = renderer.render_seconds(1.0).unwrap();
    assert_eq!(
        frame
            .fixtures
            .iter()
            .map(|fixture| fixture.fixture_id)
            .collect::<Vec<_>>(),
        (1..=30).collect::<Vec<_>>()
    );
    assert!(
        frame
            .fixtures
            .iter()
            .all(|fixture| fixture.pixels.len() == 113)
    );
}

fn checksum_frame(frame: &crate::RenderedFrame) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    hash = checksum_u64(hash, u64::from(frame.frame_index));
    for fixture in &frame.fixtures {
        hash = checksum_u32(hash, fixture.fixture_id);
        for color in &fixture.pixels {
            for channel in [color.red, color.green, color.blue] {
                hash = checksum_u8(hash, channel);
            }
        }
    }
    hash
}

fn checksum_u64(hash: u64, value: u64) -> u64 {
    value.to_le_bytes().into_iter().fold(hash, checksum_u8)
}

fn checksum_u32(hash: u64, value: u32) -> u64 {
    value.to_le_bytes().into_iter().fold(hash, checksum_u8)
}

fn checksum_u8(hash: u64, value: u8) -> u64 {
    (hash ^ u64::from(value)).wrapping_mul(0x0000_0100_0000_01b3)
}
