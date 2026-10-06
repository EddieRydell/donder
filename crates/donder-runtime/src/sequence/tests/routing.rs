//! Representation-specific target routing checks.
use super::*;
use crate::dsl::bytecode::{ContextRead, Input, SignalPixel};

#[test]
fn identical_target_routing_matches_address_search() {
    let mut data = queried_sequence(SignalPixel::Current).archive_data();
    data.signals.effects = vec![data.signals.effects[0].clone()].into();
    data.signals.effects[0].target = 0;
    data.signals.effects_by_layer[0] = vec![0].into();
    data.signals.programs[0] = program(
        vec![
            Instruction::Context {
                dst: Slot::scalar(0),
                read: ContextRead::Progress,
            },
            Instruction::FloatConst {
                dst: Slot::scalar(1),
                bits: 0.25f32.to_bits(),
            },
            Instruction::Rgb {
                dst: Slot::row(0),
                red: Slot::input(Input::PixelFraction),
                green: Slot::scalar(0),
                blue: Slot::scalar(1),
            },
        ],
        2,
        Slot::row(0),
        Banks {
            floats: 2,
            ..Banks::default()
        },
        Banks {
            colors: 1,
            ..Banks::default()
        },
    );
    data.signals.programs[2].code[0] = Instruction::Context {
        dst: Slot::scalar(0),
        read: ContextRead::Seconds,
    };
    let direct = PreparedSequence::from_archive(data).unwrap();
    let mut data = direct.archive_data();
    // A different ID for the same target forces the general address-search path.
    data.signals.targets = vec![data.signals.targets[0].clone(); 2].into();
    data.signals.effects[0].target = 1;
    let searched = PreparedSequence::from_archive(data).unwrap();
    let mut direct = direct.into_playback();
    let mut searched = searched.into_playback();
    for ticks in [300_000, 700_000, 400_000, 300_000, 0] {
        let time = SampleTime::from_ticks(ticks);
        let actual = direct.evaluate(time);
        let expected = searched.evaluate(time);
        assert_eq!(actual.colors(), expected.colors());
        assert!(actual.outputs().eq(expected.outputs()));
    }
}
