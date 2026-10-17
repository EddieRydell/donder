use donder_language::compiler::{compile_effects, compile_operators};
use donder_runtime::PreparedSequence;
use donder_runtime_types::sampling::byte_channel;
use donder_runtime_types::{Color, FixtureGeometry, TargetScope, sample_time_from_frame};
use donder_test_support::playback;

const FRAME_RATE: u32 = 120;

/// One fixture of three pixels lit orange from 0.2 s to 0.3 s, under `operator`.
fn sequence(operator: &str) -> PreparedSequence {
    let effect = compile_effects(
        "effect Flash { sample { if time >= 0.2 && time < 0.3 { rgb(1.0, 0.5, 0.0) } else { rgb(0.0, 0.0, 0.0) } } }",
    )
    .unwrap()
    .remove(0);
    let sample = playback::sample(&effect);
    let operator = playback::lower_operator(
        &compile_operators(operator)
            .unwrap()
            .remove(0)
            .bind(std::iter::empty())
            .unwrap(),
    );
    PreparedSequence::build(playback::timing(2_000_000), |builder| {
        let fixture = builder.fixture(0, FixtureGeometry::admit(vec![[0.0; 2]; 3].into()).unwrap());
        let target = builder.target([fixture], TargetScope::WholeTarget);
        let effect = builder.sample(&sample, builder.whole_sequence(), target);
        let layer = builder.layer(true, [effect]);
        let output = builder.operator(&operator, |_| layer);
        builder.output(Some(output))
    })
}

fn frame_colors(playback: &mut donder_runtime::SequencePlayback, frame: u32) -> Vec<Color> {
    playback
        .evaluate(sample_time_from_frame(frame, FRAME_RATE).unwrap())
        .colors()
        .to_vec()
}

#[test]
fn time_scans_carry_their_value_from_frame_to_frame() {
    let glow = "operator Glow { input source; sample { scan time source as light, previous { previous * 0.9 + light * 0.1 } } }";
    let mut playback = sequence(glow).into_playback();
    // The flash covers frames 24..36; the scan is a one-pole filter of it.
    let (mut red, mut green) = (0.0f32, 0.0f32);
    for frame in 0..120 {
        let lit = (24..36).contains(&frame);
        red = red * 0.9 + if lit { 255.0 } else { 0.0 } * 0.1;
        green = green * 0.9 + if lit { 128.0 } else { 0.0 } * 0.1;
        let expected = Color {
            red: byte_channel(red),
            green: byte_channel(green),
            blue: 0,
        };
        assert_eq!(
            frame_colors(&mut playback, frame),
            [expected; 3],
            "frame {frame}"
        );
    }
}

#[test]
fn seeks_replay_the_frames_that_can_still_show() {
    let glow = "operator Glow { input source; sample { scan time source as light, previous { previous * 0.9 + light * 0.1 } } }";
    let mut sequential = sequence(glow).into_playback();
    let frames: Vec<_> = (0..120)
        .map(|frame| frame_colors(&mut sequential, frame))
        .collect();
    let mut seeking = sequence(glow).into_playback();
    for frame in [60, 30, 100, 0, 101, 45, 45, 46] {
        let colors = frame_colors(&mut seeking, frame);
        for (actual, expected) in colors.iter().zip(&frames[frame as usize]) {
            // A replay starts from black where the dropped light is below
            // half an 8-bit step, so it can round one step lower.
            for (a, e) in [
                (actual.red, expected.red),
                (actual.green, expected.green),
                (actual.blue, expected.blue),
            ] {
                assert!(
                    a.abs_diff(e) <= 1,
                    "frame {frame}: {actual:?} vs {expected:?}"
                );
            }
        }
    }
}

#[test]
fn time_scans_read_the_sequence_frame_rate() {
    let rate = "operator Rate { input source; sample { rgb(frame_rate / 255.0, 0.0, 0.0) } }";
    let mut playback = sequence(rate).into_playback();
    assert_eq!(frame_colors(&mut playback, 10)[0].red, FRAME_RATE as u8);
}
