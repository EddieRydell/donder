use camino::Utf8PathBuf;
use dawn_elaboration::PreparedSequenceOutput;
use dawn_project_io::load_package;
use dawn_runtime::values::{SampleTime, sample_time_from_frame};

#[test]
fn reused_show_buffers_match_fresh_buffers_across_seeks_and_effect_ends() {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let session = load_package(&root).unwrap().session;
    for sequence in &session.project.root.sequences {
        let mut output = PreparedSequenceOutput::prepare(
            &session.project,
            &session.project.root.setup,
            sequence,
        )
        .unwrap();
        let show = &mut output.sequence;
        let mut workspace = show.workspace();
        let mut buffers = show
            .output_widths
            .iter()
            .map(|&width| vec![0; width as usize])
            .collect::<Vec<_>>();
        let mut times = [9504, 8450, 0, 8494, 8398]
            .map(|frame| sample_time_from_frame(frame, show.signals.frame_rate()).unwrap())
            .to_vec();
        times.extend(
            show.signals
                .effects
                .iter()
                .filter_map(|effect| effect.start_time.checked_add_duration(effect.duration)),
        );
        times.extend([
            SampleTime::from_ticks(0),
            SampleTime::from_ticks(show.signals.duration().as_ticks()),
        ]);
        for time in times {
            show.evaluate(time, &mut buffers, &mut workspace).unwrap();
            let mut fresh = show.workspace();
            let mut expected = buffers.clone();
            show.evaluate(time, &mut expected, &mut fresh).unwrap();
            assert_eq!(buffers, expected, "{sequence:?} at {time:?}");
            assert_eq!(
                show.rendered_fixtures(&workspace).unwrap(),
                show.rendered_fixtures(&fresh).unwrap()
            );
        }
    }
}
