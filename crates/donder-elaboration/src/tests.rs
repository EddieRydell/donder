use camino::Utf8PathBuf;
use donder_project_io::load_project;

use crate::{PrepareOutputs, prepare};
use donder_language::values::{SampleTime, sample_time_from_frame};
use donder_runtime::SequenceFrame;

fn example(name: &str) -> donder_project_io::ProjectSession {
    let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples")
        .join(name);
    load_project(&path).unwrap_or_else(|error| panic!("failed to load {name}: {error}"))
}

#[test]
fn starter_prepares_and_produces_exact_controller_widths() {
    let session = example("starter");
    let sequence_id = session.project.root().sequences.first().unwrap().id();
    let renderer = prepare(&session.project, sequence_id, PrepareOutputs::All).unwrap();
    let mut playback = renderer.into_playback();
    let frame = playback.evaluate(SampleTime::from_ticks(0));
    let setup = session
        .project
        .reusable_setups()
        .get(session.project.root().setup.id())
        .unwrap();
    let expected_ports = setup
        .controllers
        .iter()
        .map(|id| session.project.controller(id.id()).unwrap().ports.len())
        .sum::<usize>();
    assert_eq!(frame.outputs().len(), expected_ports);
    for port_frame in frame.outputs() {
        let output = port_frame.output;
        let slots = port_frame.bytes;
        let controller = session
            .project
            .reusable_controllers()
            .get(setup.controllers[output.controller_index as usize].id())
            .unwrap();
        let port = controller
            .ports
            .iter()
            .find(|port| port.id.0 == output.port)
            .unwrap();
        assert_eq!(slots.len(), usize::from(port.slot_count));
    }
}

#[test]
fn starter_sequence_behavioral_checksums_run_in_the_normal_test_gate() {
    let session = example("starter");
    let sequence_id = session.project.root().sequences.get(1).unwrap().id();
    let renderer = prepare(&session.project, sequence_id, PrepareOutputs::All).unwrap();
    let time = sample_time_from_frame(3594, renderer.frame_rate()).unwrap();
    let mut playback = renderer.into_playback();
    let rendered = playback.evaluate(time);
    assert_eq!(checksum_frame(3594, &rendered), 0xaa28_e560_49eb_1e76);
}

#[test]
fn output_fixtures_preserve_layout_instance_order() {
    let session = example("starter");
    let sequence_id = session.project.root().sequences.first().unwrap().id();
    let renderer = prepare(&session.project, sequence_id, PrepareOutputs::All).unwrap();
    let time = sample_time_from_frame(renderer.frame_rate(), renderer.frame_rate()).unwrap();
    let mut playback = renderer.into_playback();
    let frame = playback.evaluate(time);
    assert_eq!(
        frame
            .fixtures()
            .map(|fixture| fixture.fixture_id)
            .collect::<Vec<_>>(),
        (1..=30).collect::<Vec<_>>()
    );
    assert!(frame.fixtures().all(|fixture| fixture.pixels.len() == 113));
}

fn checksum_frame(frame_index: u32, frame: &SequenceFrame<'_>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    hash = checksum_u64(hash, u64::from(frame_index));
    for fixture in frame.fixtures() {
        hash = checksum_u32(hash, fixture.fixture_id);
        for color in fixture.pixels {
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
