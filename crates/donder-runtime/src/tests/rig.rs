//! Rig playback: commands start and stop voices, add live marks and set
//! controls, and frames follow from the time and the commands applied.
use super::playback;
use super::std;
use crate::{PreparedRig, RigPlayback, VoiceLength};
use donder_language::compiler::ProgramConstants;
use donder_runtime_types::{
    AutomatedQuantity, AutomationMapping, Color, Curve, CurvePoint, FixtureGeometry, Marks,
    OutputEncoding, PreparedAutomation, RgbOrder, RigCommand, RigEvent, SampleDuration,
    SampleInvocation, SampleTime, Shared, TargetScope, Value,
};
use std::prelude::rust_2024::*;

const SESSION: u32 = 10_000_000;
const WHITE: Color = Color::from_bytes([255, 255, 255]);

fn at(seconds: f32) -> SampleTime {
    SampleTime::from_ticks((seconds * 1_000_000.0) as u32)
}

fn event(seconds: f32, command: RigCommand) -> RigEvent {
    RigEvent {
        time: at(seconds),
        command,
    }
}

fn color(rig: &mut RigPlayback, seconds: f32) -> Color {
    rig.evaluate(at(seconds)).colors()[0]
}

/// A one-pixel rig whose layer holds `effects`, then `register` adds voices,
/// collections and controls by effect.
fn rig(
    effects: &[SampleInvocation],
    register: impl for<'id> FnOnce(&mut crate::SequenceBuilder<'id>, &[crate::EffectHandle<'id>]),
) -> RigPlayback {
    PreparedRig::build(playback::timing(SESSION), |builder| {
        let fixture = builder.fixture(0, FixtureGeometry::admit(Box::new([[0.0, 0.0]])).unwrap());
        let target = builder.target([fixture], TargetScope::WholeTarget);
        let port = builder.port(0, 0);
        builder.route(port, target, OutputEncoding::Rgb(RgbOrder::Rgb), None);
        let window = builder.whole_sequence();
        let handles: Vec<_> = effects
            .iter()
            .map(|effect| builder.sample(effect, window, target))
            .collect();
        register(builder, &handles);
        let layer = builder.layer(true, handles.iter().copied());
        builder.output(Some(layer))
    })
    .into_playback()
}

fn white() -> SampleInvocation {
    playback::sample(&playback::compile_effect(
        "effect White { sample { #ffffff } }",
    ))
}

#[test]
fn fixed_voice_plays_its_length_from_each_start() {
    let mut rig = rig(&[white()], |builder, effects| {
        builder.voice(
            effects[0],
            VoiceLength::Fixed(SampleDuration::from_ticks(500_000)),
        );
    });
    assert_eq!(color(&mut rig, 0.5), Color::BLACK, "dormant before a start");
    rig.apply(event(1.0, RigCommand::Start { voice: 0 }));
    assert_eq!(color(&mut rig, 1.2), WHITE);
    assert_eq!(color(&mut rig, 1.6), Color::BLACK, "ends after its length");
    rig.apply(event(2.0, RigCommand::Start { voice: 0 }));
    assert_eq!(color(&mut rig, 2.4), WHITE, "a later start replays it");
}

#[test]
fn archived_rig_plays_like_the_prepared_one() {
    let prepared = PreparedRig::build(playback::timing(SESSION), |builder| {
        let fixture = builder.fixture(0, FixtureGeometry::admit(Box::new([[0.0, 0.0]])).unwrap());
        let target = builder.target([fixture], TargetScope::WholeTarget);
        let port = builder.port(0, 0);
        builder.route(port, target, OutputEncoding::Rgb(RgbOrder::Rgb), None);
        let effect = builder.sample(&white(), builder.whole_sequence(), target);
        builder.voice(effect, VoiceLength::Open);
        let layer = builder.layer(true, [effect]);
        builder.output(Some(layer))
    });
    let bytes = prepared.encode();
    let mut rig = crate::PreparedRig::decode(&bytes).unwrap().into_playback();
    rig.apply(event(1.0, RigCommand::Start { voice: 0 }));
    assert_eq!(color(&mut rig, 2.0), WHITE);
}

#[test]
fn open_voice_plays_until_stopped() {
    let mut rig = rig(&[white()], |builder, effects| {
        builder.voice(effects[0], VoiceLength::Open);
    });
    rig.apply(event(1.0, RigCommand::Start { voice: 0 }));
    assert_eq!(color(&mut rig, 4.0), WHITE);
    rig.apply(event(4.5, RigCommand::Stop { voice: 0 }));
    assert_eq!(color(&mut rig, 4.6), Color::BLACK);
    rig.apply(event(5.0, RigCommand::Stop { voice: 0 }));
    assert_eq!(
        color(&mut rig, 5.1),
        Color::BLACK,
        "stopping again changes nothing"
    );
}

#[test]
fn live_marks_reach_a_running_effect() {
    let effect = playback::compile_effect(
        "effect Flash { param beats: marks; sample { \
         let age = time - mark_last(beats, time); \
         guard age >= 0.0 && age < 0.1; #ffffff } }",
    );
    let invocation = effect
        .invoke_live(
            vec![Value::Marks(Shared::new(Marks::empty()))],
            Box::new([]),
            Box::new([0]),
        )
        .unwrap();
    let (sample, live) = invocation
        .instance(ProgramConstants::default())
        .sample_live();
    let mut rig = rig(&[sample], |builder, effects| {
        builder.live_marks([(effects[0], live[0])]);
    });
    assert_eq!(color(&mut rig, 0.5), Color::BLACK, "no marks yet");
    rig.apply(event(1.0, RigCommand::Mark { collection: 0 }));
    assert_eq!(color(&mut rig, 1.05), WHITE);
    assert_eq!(color(&mut rig, 1.2), Color::BLACK);
    rig.apply(event(3.0, RigCommand::Mark { collection: 0 }));
    assert_eq!(color(&mut rig, 3.05), WHITE, "every mark restarts it");
}

#[test]
fn live_marks_reject_reductions_bounded_by_their_length() {
    let effect = playback::compile_effect(
        "effect Count { param beats: marks; sample { \
         let total = sum for i in 0..mark_count(beats) { 1.0 }; rgb(total, total, total) } }",
    );
    assert!(
        effect
            .invoke_live(
                vec![Value::Marks(Shared::new(Marks::empty()))],
                Box::new([]),
                Box::new([0]),
            )
            .is_err()
    );
}

/// A float control binding over the whole session, for `param` (in 0..1).
fn control(param: u16) -> PreparedAutomation {
    PreparedAutomation {
        start: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(SESSION),
        curve: Shared::new(Curve {
            points: vec![CurvePoint {
                position: 0.0,
                value: 0.0,
            }],
        }),
        mapping: AutomationMapping::Float { min: 0.0, max: 1.0 },
        quantity: AutomatedQuantity::Value,
        param_index: param,
    }
}

fn controlled(source: &str) -> (SampleInvocation, Vec<u16>) {
    let effect = playback::compile_effect(source);
    let binding = control(0);
    let invocation = effect
        .invoke(vec![Value::Float(0.0)], Box::new([binding.clone()]))
        .unwrap();
    let sample = invocation.instance(ProgramConstants::default()).sample();
    // The control sets every binding lowered from its own, integrals included.
    let bindings = sample
        .automation()
        .iter()
        .enumerate()
        .filter(|(_, lowered)| Shared::ptr_eq(&lowered.curve, &binding.curve))
        .map(|(index, _)| index as u16)
        .collect();
    (sample, bindings)
}

#[test]
fn control_sets_a_parameter_while_it_plays() {
    let (sample, bindings) = controlled(
        "effect Level { param level: float in 0.0..1.0 = 0.0; sample { rgb(level, level, level) } }",
    );
    let mut rig = rig(&[sample], |builder, effects| {
        builder.control(bindings.iter().map(|&binding| (effects[0], binding)));
    });
    assert_eq!(color(&mut rig, 0.5), Color::BLACK);
    rig.apply(event(
        1.0,
        RigCommand::Control {
            control: 0,
            value: 1.0,
        },
    ));
    assert_eq!(color(&mut rig, 1.1), WHITE);
}

#[test]
fn control_keeps_an_integral_continuous() {
    let (sample, bindings) = controlled(
        "effect Travel { param speed: float in 0.0..1.0 = 0.0; sample { \
         guard integral(speed) > 0.5; #ffffff } }",
    );
    let mut rig = rig(&[sample], |builder, effects| {
        builder.control(bindings.iter().map(|&binding| (effects[0], binding)));
    });
    rig.apply(event(
        0.0,
        RigCommand::Control {
            control: 0,
            value: 1.0,
        },
    ));
    assert_eq!(color(&mut rig, 0.4), Color::BLACK, "travelled 0.4");
    assert_eq!(color(&mut rig, 0.6), WHITE, "travelled 0.6");
    rig.apply(event(
        0.6,
        RigCommand::Control {
            control: 0,
            value: 0.0,
        },
    ));
    assert_eq!(color(&mut rig, 3.0), WHITE, "stopped at 0.6, not back at 0");
}
