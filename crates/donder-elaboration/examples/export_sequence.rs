use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare};
use donder_runtime::{LoadLimits, decode_sequence, encode_sequence};
use donder_runtime::{SampleTime, sample_time_from_frame};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    assert!(
        args.len() == 2,
        "usage: export_sequence PROJECT OUTPUT.donderseq"
    );
    let project = donder_project_io::load_project(&Utf8PathBuf::from(&args[0]))
        .unwrap()
        .project;
    let id = project
        .root()
        .sequences
        .iter()
        .map(|source| source.id())
        .find(|id| {
            id.0.source()
                .is_some_and(|source| source.object() == "layer_test")
        })
        .unwrap_or(project.root().sequences[0].id());
    let setup = project.setup(project.root().setup.id()).unwrap();
    let controller = setup.controllers[0].id();
    let ports = project
        .controller(controller)
        .unwrap()
        .ports
        .iter()
        .take(4)
        .map(|port| (controller.clone(), port.id))
        .collect::<Vec<_>>();
    let prepared = prepare(&project, id, PrepareOutputs::Ports(&ports)).unwrap();
    let bytes = encode_sequence(&prepared).unwrap();
    let decoded = decode_sequence(&bytes, LoadLimits::default()).unwrap();
    let mut checksums = String::new();
    let mut times = [0, 7150, 7151, 8398, 8450, 8494, 9504, 15000]
        .map(|frame| sample_time_from_frame(frame, prepared.frame_rate()).unwrap())
        .to_vec();
    times.extend([
        SampleTime::from_ticks(prepared.duration().as_ticks()),
        SampleTime::from_ticks(0),
    ]);
    let mut source_playback = prepared.into_playback();
    let mut playback = decoded.into_playback();
    for time in times {
        let source = source_playback.evaluate(time);
        let output = playback.evaluate(time);
        assert!(source.outputs().eq(output.outputs()));
        let mut crc = crc32fast::Hasher::new();
        for port in output.outputs() {
            crc.update(port.bytes);
        }
        checksums.push_str(&format!("{} {}\n", time.as_ticks(), crc.finalize()));
    }
    std::fs::write(&args[1], &bytes).unwrap();
    std::fs::write(format!("{}.checksums", args[1]), checksums).unwrap();
    println!(
        "sequence={} ports={} pixels={} effects={} payload_bytes={}",
        id.0.root_source().object(),
        ports.len(),
        source_playback.sequence().pixel_count(),
        source_playback.sequence().effect_count(),
        bytes.len()
    );
}
