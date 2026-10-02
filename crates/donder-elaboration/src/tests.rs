use camino::Utf8PathBuf;
use donder_project_io::load_project;

use crate::{PrepareOutputs, PreparedSequence, prepare};
use donder_runtime::signal::EvaluatedFrame;
use donder_runtime::values::{SampleTime, sample_time_from_frame};

fn example(name: &str) -> donder_project_io::ProjectSession {
    let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples")
        .join(name);
    load_project(&path).unwrap_or_else(|error| panic!("failed to load {name}: {error}"))
}

#[test]
fn every_example_prepares_and_produces_exact_controller_widths() {
    for name in ["starter"] {
        let session = example(name);
        let sequence_id = session.project.root.sequences.first().unwrap().id();
        let renderer = prepare(&session.project, sequence_id, PrepareOutputs::All)
            .unwrap_or_else(|| panic!("failed to prepare {name}"));
        let mut workspace = renderer.workspace().unwrap();
        let mut buffers = buffers(&renderer);
        renderer
            .evaluate(SampleTime::from_ticks(0), &mut buffers, &mut workspace)
            .unwrap();
        let setup = session
            .project
            .setups
            .get(session.project.root.setup.id())
            .unwrap();
        let expected_ports = setup
            .controllers
            .iter()
            .map(|id| session.project.controller(id.id()).unwrap().ports.len())
            .sum::<usize>();
        assert_eq!(buffers.len(), expected_ports);
        for (output, slots) in renderer.outputs().iter().zip(&buffers) {
            let controller = session
                .project
                .controllers
                .get(setup.controllers[output.controller_index].id())
                .unwrap();
            let port = controller
                .ports
                .iter()
                .find(|port| port.id.0 == output.port)
                .unwrap();
            assert_eq!(slots.len(), usize::from(port.slot_count));
        }
    }
}

#[test]
fn preview_and_controller_buffers_are_from_one_deterministic_show_frame() {
    let session = example("starter");
    let sequence_id = session.project.root.sequences.first().unwrap().id();
    let renderer = prepare(&session.project, sequence_id, PrepareOutputs::All).unwrap();
    let time = sample_time_from_frame(10, renderer.signals().frame_rate).unwrap();
    let mut workspace = renderer.workspace().unwrap();
    let mut first = buffers(&renderer);
    renderer.evaluate(time, &mut first, &mut workspace).unwrap();
    let first_fixtures = renderer.rendered_fixtures(&workspace).unwrap();
    let mut second = buffers(&renderer);
    renderer
        .evaluate(time, &mut second, &mut workspace)
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first_fixtures,
        renderer.rendered_fixtures(&workspace).unwrap()
    );
    assert!(!first_fixtures.is_empty());
    assert!(!first.is_empty());
    let signal_frame = renderer.signals().evaluate_frame(10).unwrap();
    assert_eq!(first_fixtures, signal_frame.fixtures);
}

#[test]
fn starter_sequence_behavioral_checksums_run_in_the_normal_test_gate() {
    let session = example("starter");
    let sequence_id = session.project.root.sequences.get(1).unwrap().id();
    let renderer = prepare(&session.project, sequence_id, PrepareOutputs::All).unwrap();
    let rendered = renderer.signals().evaluate_frame(3594).unwrap();
    assert_eq!(checksum_frame(&rendered), 0xaa28_e560_49eb_1e76);
}

#[test]
fn output_fixtures_preserve_layout_instance_order() {
    let session = example("starter");
    let sequence_id = session.project.root.sequences.first().unwrap().id();
    let renderer = prepare(&session.project, sequence_id, PrepareOutputs::All).unwrap();
    let frame = renderer
        .signals()
        .evaluate_frame(renderer.signals().frame_rate)
        .unwrap();
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

fn checksum_frame(frame: &EvaluatedFrame) -> u64 {
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

fn buffers(sequence: &PreparedSequence) -> Vec<Vec<u8>> {
    sequence
        .outputs()
        .iter()
        .map(|output| vec![0; output.width as usize])
        .collect()
}
