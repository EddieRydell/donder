//! Representation-specific target routing checks.
use super::*;
use crate::dsl::bytecode::{ColorSlot, Instruction, SignalPixel};

#[test]
fn identical_target_routing_matches_address_search() {
    use crate::dsl::bytecode::{ContextRead, FloatSlot, NumberSlot};
    let mut data = queried_sequence(SignalPixel::Current).archive_data();
    data.signals.effects = vec![data.signals.effects[0].clone()].into();
    data.signals.effects[0].target = 0;
    data.signals.effects_by_layer[0] = vec![0].into();
    let program = &mut data.signals.programs[0];
    program.layout.floats = 3;
    program.uses_pixel_context = true;
    program.instructions = vec![
        Instruction::ContextRead {
            dst: NumberSlot::Float(FloatSlot(0)),
            read: ContextRead::PixelFraction,
        },
        Instruction::ContextRead {
            dst: NumberSlot::Float(FloatSlot(1)),
            read: ContextRead::Progress,
        },
        Instruction::LoadFloatConst {
            dst: FloatSlot(2),
            bits: 0.25f32.to_bits(),
        },
        Instruction::Rgb {
            dst: ColorSlot(0),
            red: FloatSlot(0),
            green: FloatSlot(1),
            blue: FloatSlot(2),
        },
        Instruction::ReturnColor(ColorSlot(0)),
    ]
    .into();
    data.signals.programs[2].instructions[0] = Instruction::ContextRead {
        dst: NumberSlot::Float(FloatSlot(0)),
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
