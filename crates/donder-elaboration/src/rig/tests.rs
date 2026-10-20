//! Arming the starter's jam rig, and routing its keyboard buttons.
use super::{
    ArmedRig, RigRouter, RoutedAction, RoutedLength, RoutedMapping, RoutedNotes, RoutedVoices, arm,
};
use crate::{PreparationCache, PrepareOutputs};
use donder_model::{ControlRef, DonderProject};
use donder_runtime_types::{Color, RigCommand, SampleDuration, SampleTime};

fn starter() -> DonderProject {
    let path = camino::Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    donder_project_io::load_project(&path).unwrap().project
}

fn armed(project: &DonderProject) -> ArmedRig {
    let rig = project.root().rigs.first().unwrap().id();
    arm(
        project,
        rig,
        PrepareOutputs::All,
        &mut PreparationCache::default(),
    )
    .unwrap()
}

/// The router's index of the starter keyboard's button named `name`.
fn button(project: &DonderProject, armed: &ArmedRig, name: &str) -> usize {
    armed
        .buttons
        .iter()
        .position(|ControlRef { device, control }| {
            project
                .input_device(device)
                .and_then(|device| device.control(*control))
                .is_some_and(|control| control.name.as_str() == name)
        })
        .unwrap()
}

fn at(seconds: f32) -> SampleTime {
    SampleTime::from_ticks((seconds * 1_000_000.0) as u32)
}

fn commands(events: Vec<donder_runtime_types::RigEvent>) -> Vec<RigCommand> {
    events.into_iter().map(|event| event.command).collect()
}

#[test]
fn jam_arms_with_a_voice_pool_per_trigger() {
    let project = starter();
    let armed = armed(&project);
    // flash (1) + burst (stack of 4) + wash (1).
    assert_eq!(armed.prepared.voice_count(), 6);
    assert_eq!(armed.prepared.collection_count(), 1);
    assert_eq!(armed.prepared.control_count(), 1);
    // kick, swell, boost, tap and the shift switch.
    assert_eq!(armed.buttons.len(), 5);
}

#[test]
fn jam_keys_light_the_outputs() {
    let project = starter();
    let mut armed = armed(&project);
    let kick = button(&project, &armed, "kick");
    let mut playback = armed.prepared.clone().into_playback();
    let dark = |frame: &[Color]| frame.iter().all(|color| *color == Color::BLACK);
    assert!(
        dark(playback.evaluate(at(0.5)).colors()),
        "nothing plays yet"
    );
    for event in armed.router.press(kick, at(1.0)) {
        playback.apply(event);
    }
    assert!(!dark(playback.evaluate(at(1.1)).colors()), "kick flashes");
    assert!(dark(playback.evaluate(at(1.6)).colors()), "the flash ends");
}

#[test]
fn switches_page_mappings_and_stacks_take_voices_in_turn() {
    let project = starter();
    let mut armed = armed(&project);
    let kick = button(&project, &armed, "kick");
    let shift = button(&project, &armed, "shift");
    let router = &mut armed.router;
    let start = |events: Vec<_>| match commands(events).as_slice() {
        [RigCommand::Start { voice }] => *voice,
        other => panic!("expected one start, got {other:?}"),
    };
    let flash = start(router.press(kick, at(1.0)));
    router.release(kick, at(1.1));
    assert!(commands(router.press(shift, at(2.0))).is_empty());
    router.release(shift, at(2.1));
    let first = start(router.press(kick, at(3.0)));
    router.release(kick, at(3.1));
    let second = start(router.press(kick, at(4.0)));
    router.release(kick, at(4.1));
    assert_ne!(flash, first, "page two plays the burst, not the flash");
    assert_ne!(first, second, "a stack takes its next voice");
}

#[test]
fn held_and_control_buttons_answer_their_release() {
    let project = starter();
    let mut armed = armed(&project);
    let swell = button(&project, &armed, "swell");
    let boost = button(&project, &armed, "boost");
    let tap = button(&project, &armed, "tap");
    let router = &mut armed.router;
    let [RigCommand::Start { voice }] = commands(router.press(swell, at(1.0)))[..] else {
        panic!("a held trigger starts on press");
    };
    assert!(
        commands(router.press(swell, at(1.2))).is_empty(),
        "key repeat"
    );
    assert_eq!(
        commands(router.release(swell, at(2.0))),
        [RigCommand::Stop { voice }]
    );
    assert!(matches!(
        commands(router.press(boost, at(3.0)))[..],
        [RigCommand::Control { value: 1.0, .. }]
    ));
    assert!(matches!(
        commands(router.release(boost, at(3.5)))[..],
        [RigCommand::Control { value: 0.0, .. }]
    ));
    assert!(matches!(
        commands(router.press(tap, at(4.0)))[..],
        [RigCommand::Mark { .. }]
    ));
}

#[test]
fn mono_ignores_presses_while_its_voice_plays_and_latch_toggles() {
    let mut router = RigRouter::new(
        Box::new([RoutedNotes::None; 2]),
        Box::new([]),
        Box::new([
            RoutedMapping {
                button: 0,
                when: None,
                action: RoutedAction::Trigger {
                    voices: Box::new([0]),
                    length: RoutedLength::Fixed(SampleDuration::from_ticks(1_000_000)),
                    policy: RoutedVoices::Mono,
                    values: Box::new([]),
                },
            },
            RoutedMapping {
                button: 1,
                when: None,
                action: RoutedAction::Trigger {
                    voices: Box::new([1]),
                    length: RoutedLength::Latched,
                    policy: RoutedVoices::Retrigger,
                    values: Box::new([]),
                },
            },
        ]),
    );
    assert_eq!(
        commands(router.press(0, at(0.0))),
        [RigCommand::Start { voice: 0 }]
    );
    router.release(0, at(0.1));
    assert!(
        commands(router.press(0, at(0.5))).is_empty(),
        "still playing"
    );
    router.release(0, at(0.6));
    assert_eq!(
        commands(router.press(0, at(1.5))),
        [RigCommand::Start { voice: 0 }]
    );
    assert_eq!(
        commands(router.press(1, at(2.0))),
        [RigCommand::Start { voice: 1 }]
    );
    assert!(
        commands(router.release(1, at(2.1))).is_empty(),
        "a latch outlasts its press"
    );
    assert_eq!(
        commands(router.press(1, at(3.0))),
        [RigCommand::Stop { voice: 1 }]
    );
}
