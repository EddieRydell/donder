//! Console playback: commands start and stop voices, add live marks and set
//! live automations, and frames follow from the time and the commands applied.
use super::evaluation::whole;
use super::playback;
use super::std;
use crate::{PreparedShow, ShowPlayback};
use donder_language::compiler::ProgramConstants;
use donder_runtime_types::{
    AutomatedQuantity, AutomationMapping, BoundProgram, Color, ConsoleCommand, ConsoleEvent, Curve,
    CurvePoint, FixtureGeometry, Marks, OutputEncoding, PreparedAutomation, RgbOrder,
    SampleDuration, SampleTime, Shared, TargetScope, Value,
};
use std::prelude::rust_2024::*;

const SESSION: u32 = 10_000_000;
const WHITE: Color = Color::from_bytes([255, 255, 255]);

fn at(seconds: f32) -> SampleTime {
    SampleTime::from_ticks((seconds * 1_000_000.0) as u32)
}

fn event(seconds: f32, command: ConsoleCommand) -> ConsoleEvent {
    ConsoleEvent {
        time: at(seconds),
        command,
    }
}

fn color(console: &mut ShowPlayback, seconds: f32) -> Color {
    console.evaluate(at(seconds)).colors()[0]
}

/// A one-pixel console whose layer holds `effects`, then `register` adds
/// voices, collections and live automations by effect.
fn console(
    effects: &[BoundProgram],
    register: impl for<'id> FnOnce(&mut crate::ShowBuilder<'id>, &[crate::ClipHandle<'id>]),
) -> ShowPlayback {
    PreparedShow::build(playback::timing(SESSION), |builder| {
        let geometry = FixtureGeometry::admit(Box::new([[0.0, 0.0]])).unwrap();
        builder.fixture(0, geometry.clone());
        let target = whole(builder, &geometry, TargetScope::WholeTarget);
        let port = builder.port(0, 0);
        builder.route(port, target, OutputEncoding::Rgb(RgbOrder::Rgb), None);
        let window = SampleDuration::from_ticks(SESSION);
        let handles: Vec<_> = effects
            .iter()
            .map(|effect| builder.clip(None, effect, SampleTime::from_ticks(0), window, target))
            .collect();
        register(builder, &handles);
        let layer = builder.layer(true, handles.iter().copied());
        Some(layer)
    })
    .into_playback()
}

fn white() -> BoundProgram {
    playback::program(&playback::compile("effect White { sample { #ffffff } }"))
}

#[test]
fn fixed_voice_plays_its_length_from_each_start() {
    let mut console = console(&[white()], |builder, effects| {
        builder.voice(effects[0]);
    });
    assert_eq!(
        color(&mut console, 0.5),
        Color::BLACK,
        "dormant before a start"
    );
    console.apply(event(
        1.0,
        ConsoleCommand::Start {
            voice: 0,
            duration: Some(SampleDuration::from_ticks(500_000)),
        },
    ));
    assert_eq!(color(&mut console, 1.2), WHITE);
    assert_eq!(
        color(&mut console, 1.6),
        Color::BLACK,
        "ends after its length"
    );
    console.apply(event(
        2.0,
        ConsoleCommand::Start {
            voice: 0,
            duration: Some(SampleDuration::from_ticks(500_000)),
        },
    ));
    assert_eq!(color(&mut console, 2.4), WHITE, "a later start replays it");
}

#[test]
fn archived_console_plays_like_the_prepared_one() {
    let prepared = PreparedShow::build(playback::timing(SESSION), |builder| {
        let geometry = FixtureGeometry::admit(Box::new([[0.0, 0.0]])).unwrap();
        builder.fixture(0, geometry.clone());
        let target = whole(builder, &geometry, TargetScope::WholeTarget);
        let port = builder.port(0, 0);
        builder.route(port, target, OutputEncoding::Rgb(RgbOrder::Rgb), None);
        let effect = builder.clip(
            None,
            &white(),
            SampleTime::from_ticks(0),
            SampleDuration::from_ticks(SESSION),
            target,
        );
        builder.voice(effect);
        let layer = builder.layer(true, [effect]);
        Some(layer)
    });
    let bytes = prepared.encode();
    let mut console = crate::PreparedShow::decode(&bytes).unwrap().into_playback();
    console.apply(event(
        1.0,
        ConsoleCommand::Start {
            voice: 0,
            duration: None,
        },
    ));
    assert_eq!(color(&mut console, 2.0), WHITE);
}

#[test]
fn open_voice_plays_until_stopped() {
    let mut console = console(&[white()], |builder, effects| {
        builder.voice(effects[0]);
    });
    console.apply(event(
        1.0,
        ConsoleCommand::Start {
            voice: 0,
            duration: None,
        },
    ));
    assert_eq!(color(&mut console, 4.0), WHITE);
    console.apply(event(4.5, ConsoleCommand::Stop { voice: 0 }));
    assert_eq!(color(&mut console, 4.6), Color::BLACK);
    console.apply(event(5.0, ConsoleCommand::Stop { voice: 0 }));
    assert_eq!(
        color(&mut console, 5.1),
        Color::BLACK,
        "stopping again changes nothing"
    );
}

#[test]
fn live_marks_reach_a_running_effect() {
    let effect = playback::compile(
        "effect Flash { param beats: marks; sample { \
         let age = time - mark_last(beats, time); \
         guard age >= 0.0 && age < 0.1; #ffffff } }",
    );
    let invocation = effect
        .invoke(
            vec![Value::Marks(Shared::new(Marks::empty()))],
            Box::new([live_marks(0)]),
        )
        .unwrap();
    let sample = playback::bind(&invocation.instance(ProgramConstants::default()));
    let mut console = console(&[sample], |builder, effects| {
        builder.live_marks([(effects[0], 0)]);
    });
    assert_eq!(color(&mut console, 0.5), Color::BLACK, "no marks yet");
    console.apply(event(1.0, ConsoleCommand::Mark { collection: 0 }));
    assert_eq!(color(&mut console, 1.05), WHITE);
    assert_eq!(color(&mut console, 1.2), Color::BLACK);
    console.apply(event(3.0, ConsoleCommand::Mark { collection: 0 }));
    assert_eq!(color(&mut console, 3.05), WHITE, "every mark restarts it");
}

#[test]
fn live_marks_reject_reductions_bounded_by_their_length() {
    let effect = playback::compile(
        "effect Count { param beats: marks; sample { \
         let total = sum for i in 0..mark_count(beats) { 1.0 }; rgb(total, total, total) } }",
    );
    assert!(
        effect
            .invoke(
                vec![Value::Marks(Shared::new(Marks::empty()))],
                Box::new([live_marks(0)]),
            )
            .is_err()
    );
}

/// A float live automation binding over the whole session, for `param` (in 0..1).
fn automation(param: u16) -> PreparedAutomation {
    session(AutomationMapping::Float { min: 0.0, max: 1.0 }, param)
}

/// A binding of marks parameter `param` to a live collection.
fn live_marks(param: u16) -> PreparedAutomation {
    session(AutomationMapping::LiveMarks, param)
}

fn session(mapping: AutomationMapping, param: u16) -> PreparedAutomation {
    PreparedAutomation {
        start: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(SESSION),
        curve: Shared::new(Curve {
            points: vec![CurvePoint {
                position: 0.0,
                value: 0.0,
            }],
        }),
        mapping,
        quantity: AutomatedQuantity::Value,
        param_index: param,
    }
}

fn automated(source: &str) -> (BoundProgram, Vec<u16>) {
    let effect = playback::compile(source);
    let binding = automation(0);
    let invocation = effect
        .invoke(vec![Value::Float(0.0)], Box::new([binding.clone()]))
        .unwrap();
    let sample = playback::bind(&invocation.instance(ProgramConstants::default()));
    // The live automation sets every binding lowered from its own, integrals included.
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
fn set_changes_a_parameter_while_it_plays() {
    let (sample, bindings) = automated(
        "effect Level { param level: float in 0.0..1.0 = 0.0; sample { rgb(level, level, level) } }",
    );
    let mut console = console(&[sample], |builder, effects| {
        builder.live_automation(bindings.iter().map(|&binding| (effects[0], binding)));
    });
    assert_eq!(color(&mut console, 0.5), Color::BLACK);
    console.apply(event(
        1.0,
        ConsoleCommand::Set {
            automation: 0,
            value: 1.0,
        },
    ));
    assert_eq!(color(&mut console, 1.1), WHITE);
}

#[test]
fn set_keeps_an_integral_continuous() {
    let (sample, bindings) = automated(
        "effect Travel { param speed: float in 0.0..1.0 = 0.0; sample { \
         guard integral(speed) > 0.5; #ffffff } }",
    );
    let mut console = console(&[sample], |builder, effects| {
        builder.live_automation(bindings.iter().map(|&binding| (effects[0], binding)));
    });
    console.apply(event(
        0.0,
        ConsoleCommand::Set {
            automation: 0,
            value: 1.0,
        },
    ));
    assert_eq!(color(&mut console, 0.4), Color::BLACK, "travelled 0.4");
    assert_eq!(color(&mut console, 0.6), WHITE, "travelled 0.6");
    console.apply(event(
        0.6,
        ConsoleCommand::Set {
            automation: 0,
            value: 0.0,
        },
    ));
    assert_eq!(
        color(&mut console, 3.0),
        WHITE,
        "stopped at 0.6, not back at 0"
    );
}
