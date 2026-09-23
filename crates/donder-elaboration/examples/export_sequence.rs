use camino::Utf8PathBuf;
use donder_elaboration::PreparedSequenceOutput;
use donder_runtime::values::{SampleTime, sample_time_from_frame};
use donder_runtime::wire::{LoadLimits, decode_sequence, encode_sequence};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    assert!(
        args.len() == 2,
        "usage: export_sequence PROJECT OUTPUT.donderseq"
    );
    let project = donder_project_io::load_package(&Utf8PathBuf::from(&args[0]))
        .unwrap()
        .session
        .project;
    let id = project
        .root
        .sequences
        .iter()
        .find(|id| id.0.object() == "layer_test")
        .unwrap_or(&project.root.sequences[0]);
    let setup = &project.setups[&project.root.setup];
    let controller = &setup.controllers[0];
    let ports = project.controllers[controller]
        .ports
        .iter()
        .take(4)
        .map(|port| (controller.clone(), port.id))
        .collect::<Vec<_>>();
    let prepared =
        PreparedSequenceOutput::prepare_selected(&project, &project.root.setup, id, &ports)
            .unwrap();
    let sequence = &prepared.sequence;
    let bytes = encode_sequence(sequence).unwrap();
    let decoded = decode_sequence(&bytes, LoadLimits::default()).unwrap();
    let mut source_workspace = sequence.workspace();
    let mut workspace = decoded.workspace();
    let mut source = sequence
        .output_widths
        .iter()
        .map(|&width| vec![0; width as usize])
        .collect::<Vec<_>>();
    let mut output = source.clone();
    let mut checksums = String::new();
    let mut times = [0, 7150, 7151, 8398, 8450, 8494, 9504, 15000]
        .map(|frame| sample_time_from_frame(frame, sequence.signals.frame_rate).unwrap())
        .to_vec();
    times.extend([
        SampleTime::from_ticks(sequence.signals.duration.as_ticks()),
        SampleTime::from_ticks(0),
    ]);
    for time in times {
        sequence
            .evaluate(time, &mut source, &mut source_workspace)
            .unwrap();
        decoded.evaluate(time, &mut output, &mut workspace).unwrap();
        assert_eq!(source, output);
        let mut crc = crc32fast::Hasher::new();
        for bytes in &output {
            crc.update(bytes);
        }
        checksums.push_str(&format!("{} {}\n", time.as_ticks(), crc.finalize()));
    }
    std::fs::write(&args[1], &bytes).unwrap();
    std::fs::write(format!("{}.checksums", args[1]), checksums).unwrap();
    println!(
        "sequence={} ports={} pixels={} effects={} payload_bytes={}",
        id.0.object(),
        ports.len(),
        sequence.signals.pixel_count,
        sequence.signals.effects.len(),
        bytes.len()
    );
}
