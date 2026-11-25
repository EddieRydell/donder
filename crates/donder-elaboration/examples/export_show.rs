use camino::Utf8PathBuf;
use donder_elaboration::{Retain, prepare};
use donder_runtime_types::{SampleTime, sample_time_from_frame};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    assert!(
        args.len() == 2,
        "usage: export_show PROJECT OUTPUT.dondershow"
    );
    let session = donder_project_io::check_project(
        &Utf8PathBuf::from(&args[0]),
        &Default::default(),
        &mut Default::default(),
    )
    .into_session()
    .unwrap();
    let project = session.project();
    let id = project
        .reusable::<donder_model::Sequence>()
        .keys()
        .find(|id| {
            session
                .source()
                .declared_name((**id).into())
                .is_some_and(|name| name.as_str() == "layer_test")
        })
        .unwrap_or(
            project
                .reusable::<donder_model::Sequence>()
                .get_index(0)
                .unwrap()
                .0,
        );
    let controllers_store = project.reusable::<donder_model::Controller>();
    let controller = controllers_store.get_index(0).unwrap().0;
    let outputs = project
        .get::<donder_model::Controller>(*controller)
        .unwrap()
        .outputs()
        .into_iter()
        .take(4)
        .map(|(output, _)| (*controller, output))
        .collect::<Vec<_>>();
    let prepared = prepare(project, id, Retain::Outputs(&outputs)).unwrap();
    let bytes = prepared.encode();
    let decoded = donder_runtime::PreparedShow::decode(&bytes).unwrap();
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
    fs_err::write(&args[1], &bytes).unwrap();
    fs_err::write(format!("{}.checksums", args[1]), checksums).unwrap();
    println!(
        "sequence={} outputs={} pixels={} effects={} payload_bytes={}",
        session
            .source()
            .declared_name((*id).into())
            .map_or_else(|| "owned".to_string(), |name| name.as_str().to_string()),
        outputs.len(),
        source_playback.prepared().pixel_count(),
        source_playback.prepared().clip_count(),
        bytes.len()
    );
}
