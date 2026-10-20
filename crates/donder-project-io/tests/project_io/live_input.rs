//! Input devices and rigs: the starter's keyboard and jam rig, and the
//! diagnostics for invalid ones.
use crate::common;

use camino::Utf8PathBuf;
use donder_model::{
    ControlTarget, InputControlKind, MappingAction, TriggerLength, VoicePolicy, When,
};
use donder_project_io::{check_project_with_overrides, project_source_texts};

const KEYBOARD: &str = "input_devices/keyboard.data.donder";
const JAM: &str = "rigs/jam.data.donder";
const SETUP: &str = "setups/main.data.donder";

#[test]
fn starter_keyboard_and_jam_rig_load_typed() {
    let session = common::load_project(&common::starter_root());
    let project = &session.project;
    let setup = project.setup(project.root().setup.id()).unwrap();
    let [device] = setup.input_devices.as_slice() else {
        panic!("the starter setup lists one input device");
    };
    let device = project.input_device(device.id()).unwrap();
    assert_eq!(device.controls.len(), 5);
    assert!(
        device
            .controls
            .iter()
            .all(|control| matches!(control.kind, InputControlKind::Button(_)))
    );
    let [rig] = project.root().rigs.as_slice() else {
        panic!("the starter project lists one rig");
    };
    let rig = project.rig(rig.id()).unwrap();
    assert_eq!(rig.live_marks.len(), 1);
    assert_eq!(rig.voices.len(), 1);
    assert_eq!(rig.switches.len(), 1);
    assert!(rig.inputs().all(|input| input.device == device.id));
    let actions = rig
        .mappings
        .iter()
        .map(|mapping| (mapping.when, &mapping.action))
        .collect::<Vec<_>>();
    let switch = rig.switches[0].id;
    assert!(matches!(
        actions.as_slice(),
        [
            (
                Some(When::Off(off)),
                MappingAction::Trigger(flash)
            ),
            (Some(When::On(on)), MappingAction::Trigger(burst)),
            (None, MappingAction::Trigger(wash)),
            (None, MappingAction::Mark(_)),
            (
                None,
                MappingAction::Control {
                    target: ControlTarget::Voice(_),
                    ..
                }
            ),
        ] if *off == switch
            && *on == switch
            && matches!(flash.length, TriggerLength::Fixed(_))
            && flash.voices == VoicePolicy::Retrigger
            && burst.voices == VoicePolicy::Stack { count: 4 }
            && wash.length == TriggerLength::Held
            && wash.voices == VoicePolicy::Mono
    ));
    donder_model::validate_project(project).unwrap();
}

#[test]
fn starter_keyboard_and_jam_rig_save_byte_exactly() {
    let (_temporary, root) = common::starter_copy();
    let before = project_source_texts(&root).unwrap();
    let session = common::load_project(&root);
    donder_project_io::save_project(&session).unwrap();
    let after = project_source_texts(&root).unwrap();
    for path in [KEYBOARD, JAM, SETUP, donder_project_io::PROJECT_ROOT_FILE] {
        let path = Utf8PathBuf::from(path);
        assert_eq!(before[&path], after[&path], "{path}");
    }
    assert_eq!(session.project, common::load_project(&root).project);
}

/// Each edit of a starter document is rejected with a ranged diagnostic
/// containing `expected`.
fn assert_rejected(edits: &[(&str, &str, &str, &str)]) {
    let (_temporary, root) = common::starter_copy();
    let original = project_source_texts(&root).unwrap();
    for (path, before, after, expected) in edits {
        let path = Utf8PathBuf::from(*path);
        assert!(original[&path].contains(before), "{before}");
        let mut overrides = original.clone();
        overrides.insert(path.clone(), original[&path].replacen(before, after, 1));
        let report = check_project_with_overrides(&root, &overrides);
        assert!(report.session.is_none(), "{after} was accepted");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)
                    && diagnostic.range.is_some()),
            "{after}: {:?}",
            report.diagnostics
        );
    }
}

#[test]
fn invalid_input_devices_are_rejected_at_the_control() {
    assert_rejected(&[
        (
            KEYBOARD,
            "Button { name: swell,",
            "Button { name: kick,",
            "control `kick` is declared twice",
        ),
        (
            KEYBOARD,
            "code: KeyS",
            "code: KeyA",
            "control `swell` reads `KeyA`, which control `kick` already reads",
        ),
        (
            KEYBOARD,
            "code: KeyS",
            "code: Key5",
            "`Key5` is not a key code",
        ),
        (
            KEYBOARD,
            "driver: Keyboard",
            "driver: Gamepad",
            "expected one of `Keyboard`, `Midi { ... }`",
        ),
    ]);
}

#[test]
fn invalid_rigs_are_rejected_at_the_reference() {
    assert_rejected(&[
        (
            JAM,
            "input: devices.keyboard.tap,",
            "input: devices.keyboard,",
            "`devices.keyboard` is an input device; an input is one of its controls",
        ),
        (
            JAM,
            "input: devices.keyboard.tap,",
            "input: devices.keyboard.snare,",
            "`devices.keyboard` has no control `snare`",
        ),
        (
            JAM,
            "Stack { count: 4 }",
            "Stack { count: 17 }",
            "mapping `burst` stacks 17 voices; a stack has 1 to 16",
        ),
        (
            JAM,
            "Stack { count: 4 }",
            "Stack { count: 0 }",
            "mapping `burst` stacks 0 voices",
        ),
        (
            JAM,
            "duration: 0.4s",
            "duration: 0s",
            "mapping `flash` has a `Fixed` duration of zero",
        ),
        (
            JAM,
            "param: decay_seconds",
            "param: accent",
            "`accent` cannot be controlled",
        ),
        (
            JAM,
            "param: decay_seconds",
            "param: speed",
            "no parameter `speed`",
        ),
        (
            JAM,
            "voice: background",
            "voice: tap",
            "`tap` is not a voice or a `Trigger` mapping",
        ),
        (
            JAM,
            "On { switch: page_two }",
            "On { switch: page_three }",
            "no switch `page_three`",
        ),
        (
            JAM,
            "Mark { marks: beat }",
            "Mark { marks: downbeat }",
            "no live mark collection `downbeat`",
        ),
        (
            JAM,
            "name: wash,",
            "name: background,",
            "`background` names a voice or mapping already",
        ),
        (
            SETUP,
            "input_devices: [devices.keyboard],",
            "input_devices: [],",
            "`devices.keyboard` is not one of the project setup's `input_devices`",
        ),
    ]);
}

#[test]
fn rig_edits_are_validated_like_loading() {
    let mut session = common::load_project(&common::starter_root());
    let rig = session
        .project
        .reusable_rigs()
        .values()
        .next()
        .unwrap()
        .clone();
    let rejected = |edit: &dyn Fn(&mut donder_model::Rig), expected: &str| {
        let mut value = rig.clone();
        edit(&mut value);
        let error = session
            .project
            .clone()
            .replace_rig(&rig.id, value)
            .unwrap_err();
        assert!(error.contains(expected), "{error}");
    };
    rejected(
        &|rig| {
            let MappingAction::Trigger(trigger) = &mut rig.mappings[1].action else {
                unreachable!("`burst` triggers");
            };
            trigger.voices = VoicePolicy::Stack { count: 0 };
        },
        "a stack has 1 to 16",
    );
    rejected(
        &|rig| {
            let MappingAction::Trigger(trigger) = &mut rig.mappings[0].action else {
                unreachable!("`flash` triggers");
            };
            trigger.length =
                TriggerLength::Fixed(donder_language::DonderDuration(std::time::Duration::ZERO));
        },
        "`Fixed` duration that is not positive",
    );
    rejected(
        &|rig| {
            let MappingAction::Control { param, .. } = &mut rig.mappings[4].action else {
                unreachable!("`boost` controls");
            };
            *param = donder_language::object_name("accent");
        },
        "cannot be automated",
    );
    rejected(
        &|rig| rig.mappings[2].name = rig.voices[0].name.clone(),
        "voice and mapping names must be unique",
    );
    // A rig of the project reads only the input devices of the project's setup.
    let setup_id = session.project.root().setup.id().clone();
    let mut setup = session.project.setup(&setup_id).unwrap().clone();
    setup.input_devices.clear();
    let error = session.project.replace_setup(&setup_id, setup).unwrap_err();
    assert!(
        error.contains("an input device is not in the project's setup"),
        "{error}"
    );
}
