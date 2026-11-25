//! Console playback: commands start and stop voices, add live marks and set
//! live automations, and frames follow from the time and the commands applied.
use super::evaluation::whole;
use super::playback;
use super::std;
use crate::playback::ConsoleCommand;
use crate::{PreparedShow, ShowPlayback};
use donder_language::compiler::{LiveWords, ProgramConstants};
use donder_runtime_types::{
    AutomationMapping, AutomationSource, BoundProgram, Color, FixtureGeometry, Marks,
    OutputEncoding, PixelEncoding, PreparedAutomation, SampleDuration, SampleTime, Shared,
    TargetScope, Value,
};
use std::prelude::rust_2024::*;

const SESSION: u32 = 10_000_000;
const WHITE: Color = Color::from_bytes([255, 255, 255]);

fn at(seconds: f32) -> SampleTime {
    SampleTime::from_ticks((seconds * 1_000_000.0) as u32)
}

fn event(seconds: f32, command: ConsoleCommand) -> (SampleTime, ConsoleCommand) {
    (at(seconds), command)
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
        let geometry =
            FixtureGeometry::admit(Box::new([[0.0, 0.0, 0.0]]), Box::new([(0, 0)])).unwrap();
        builder.fixture(0, geometry.clone());
        let target = whole(builder, &geometry, TargetScope::WholeTarget);
        let port = builder.port(0, 0);
        builder.route(
            port,
            target,
            OutputEncoding::admit(PixelEncoding::Rgb { order: [0, 1, 2] }).unwrap(),
            None,
            false,
            false,
        );
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
    console.command(event(
        1.0,
        ConsoleCommand::Start {
            voice: 0,
            duration: SampleDuration::from_ticks(500_000),
        },
    ));
    assert_eq!(color(&mut console, 1.2), WHITE);
    assert_eq!(
        color(&mut console, 1.6),
        Color::BLACK,
        "ends after its length"
    );
    console.command(event(
        2.0,
        ConsoleCommand::Start {
            voice: 0,
            duration: SampleDuration::from_ticks(500_000),
        },
    ));
    assert_eq!(color(&mut console, 2.4), WHITE, "a later start replays it");
}

#[test]
fn archived_console_plays_like_the_prepared_one() {
    let prepared = PreparedShow::build(playback::timing(SESSION), |builder| {
        let geometry =
            FixtureGeometry::admit(Box::new([[0.0, 0.0, 0.0]]), Box::new([(0, 0)])).unwrap();
        builder.fixture(0, geometry.clone());
        let target = whole(builder, &geometry, TargetScope::WholeTarget);
        let port = builder.port(0, 0);
        builder.route(
            port,
            target,
            OutputEncoding::admit(PixelEncoding::Rgb { order: [0, 1, 2] }).unwrap(),
            None,
            false,
            false,
        );
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
    console.command(event(
        1.0,
        ConsoleCommand::Start {
            voice: 0,
            duration: SampleDuration::from_ticks(10_000_000),
        },
    ));
    assert_eq!(color(&mut console, 2.0), WHITE);
}

#[test]
fn voice_plays_until_stopped() {
    let mut console = console(&[white()], |builder, effects| {
        builder.voice(effects[0]);
    });
    console.command(event(
        1.0,
        ConsoleCommand::Start {
            voice: 0,
            duration: SampleDuration::from_ticks(10_000_000),
        },
    ));
    assert_eq!(color(&mut console, 4.0), WHITE);
    console.command(event(4.5, ConsoleCommand::Stop { voice: 0 }));
    assert_eq!(color(&mut console, 4.6), Color::BLACK);
    console.command(event(5.0, ConsoleCommand::Stop { voice: 0 }));
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
         let age = time - mark_at(beats, mark_last_index(beats, time)); \
         guard age >= 0.0 && age < 0.1; #ffffff } }",
    );
    let invocation = effect
        .invoke(
            vec![Value::Marks(Shared::new(Marks::live(0)))],
            Box::new([live_marks(0)]),
        )
        .unwrap();
    let sample = playback::bind(&invocation.instance(ProgramConstants::default(), &[]));
    let mut console = console(&[sample], |builder, _| {
        builder.console(crate::ConsoleRouting {
            controls: 0,
            collections: 1,
            clips: Vec::new(),
            ons: Vec::new(),
            always: Vec::new(),
        });
    });
    assert_eq!(color(&mut console, 0.5), Color::BLACK, "no marks yet");
    console.command(event(1.0, ConsoleCommand::Mark { collection: 0 }));
    assert_eq!(color(&mut console, 1.05), WHITE);
    assert_eq!(color(&mut console, 1.2), Color::BLACK);
    console.command(event(3.0, ConsoleCommand::Mark { collection: 0 }));
    assert_eq!(color(&mut console, 3.05), WHITE, "every mark restarts it");
}

#[test]
fn live_marks_reject_reductions_bounded_by_their_length() {
    let effect = playback::compile(
        "effect Count { param beats: marks; sample { \
         let total = sum for i in 0..len(beats) { 1.0 }; rgb(total, total, total) } }",
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

/// A binding of parameter `param` to its invocation's first live value.
fn live(mapping: AutomationMapping, param: u16) -> PreparedAutomation {
    PreparedAutomation {
        source: AutomationSource::Live(0),
        mapping,
        param_index: param,
    }
}

/// A binding of marks parameter `param` to a live collection.
fn live_marks(param: u16) -> PreparedAutomation {
    live(AutomationMapping::LiveMarks, param)
}

/// `source`'s float parameter set by the console's first live automation,
/// whose words come first in the live store, and its integral counting from
/// the snapshot after them.
fn automated(source: &str) -> BoundProgram {
    let effect = playback::compile(source);
    let binding = live(AutomationMapping::Float { min: 0.0, max: 1.0 }, 0);
    let invocation = effect
        .invoke(vec![Value::Float(0.0)], Box::new([binding]))
        .unwrap();
    let words = LiveWords {
        value: 0,
        snapshot: Some(3),
    };
    playback::bind(&invocation.instance(ProgramConstants::default(), &[words]))
}

/// The console's first live automation, resting at 0, and the snapshot its
/// clip's integrals count from.
fn live_automation<'id>(builder: &mut crate::ShowBuilder<'id>, clip: crate::ClipHandle<'id>) {
    let (automation, word) = builder.live_automation(0.0);
    assert_eq!(word, 0);
    let snapshot = builder.snapshot_word();
    builder.snapshot(clip, automation, snapshot);
}

#[test]
fn set_changes_a_parameter_while_it_plays() {
    let sample = automated(
        "effect Level { param level: float in 0.0..1.0 = 0.0; sample { rgb(level, level, level) } }",
    );
    let mut console = console(&[sample], |builder, effects| {
        live_automation(builder, effects[0]);
    });
    assert_eq!(color(&mut console, 0.5), Color::BLACK);
    console.command(event(
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
    let sample = automated(
        "effect Travel { param speed: float in 0.0..1.0 = 0.0; sample { \
         guard integral(speed) > 0.5; #ffffff } }",
    );
    let mut console = console(&[sample], |builder, effects| {
        live_automation(builder, effects[0]);
    });
    console.command(event(
        0.0,
        ConsoleCommand::Set {
            automation: 0,
            value: 1.0,
        },
    ));
    assert_eq!(color(&mut console, 0.4), Color::BLACK, "travelled 0.4");
    assert_eq!(color(&mut console, 0.6), WHITE, "travelled 0.6");
    console.command(event(
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
