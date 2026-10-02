use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare as prepare_sequence};
use donder_project_io::load_project;
use donder_runtime::{SampleTime, sample_time_from_frame};

#[test]
fn reused_show_buffers_match_fresh_buffers_across_seeks_and_effect_ends() {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let session = load_project(&root).unwrap();
    for sequence in session
        .project
        .root()
        .sequences
        .iter()
        .map(|source| source.id())
    {
        let show = prepare_sequence(&session.project, sequence, PrepareOutputs::All).unwrap();
        let mut workspace = donder_runtime::PreparedSequence::admit(
            show.to_raw_signals(),
            show.patch().clone(),
            show.outputs().into(),
        )
        .unwrap()
        .into_playback();
        let mut times = [9504, 8450, 0, 8494, 8398]
            .map(|frame| sample_time_from_frame(frame, show.frame_rate()).unwrap())
            .to_vec();
        times.extend(
            show.to_raw_signals()
                .effects
                .iter()
                .filter_map(|effect| effect.start_time.checked_add_duration(effect.duration)),
        );
        times.extend([
            SampleTime::from_ticks(0),
            SampleTime::from_ticks(show.duration().as_ticks()),
        ]);
        for time in times {
            let actual = workspace.evaluate(time);
            let mut fresh = donder_runtime::PreparedSequence::admit(
                show.to_raw_signals(),
                show.patch().clone(),
                show.outputs().into(),
            )
            .unwrap()
            .into_playback();
            let expected = fresh.evaluate(time);
            assert!(
                actual.outputs().eq(expected.outputs()),
                "{sequence:?} at {time:?}"
            );
            assert_eq!(
                actual.fixtures().collect::<Vec<_>>(),
                expected.fixtures().collect::<Vec<_>>()
            );
        }
    }
}
