//! Input devices and consoles: the starter's keyboard and jam console, and
//! the diagnostics for invalid ones.
use crate::common;

use camino::Utf8PathBuf;
use donder_model::{
    Action, AutomationTarget, CompositionGraphNodeId, ConsoleValidationError, EditError,
    EventProblem, InputControlKind, InputEvent, InputProblem, LiveAutomationProblem, Member,
    NameProblem, PlayLength, ProjectValidationError, ValidationProblem,
};
use donder_project_io::{check_project, project_source_texts};

const KEYBOARD: &str = "input_devices/keyboard.data.donder";
const JAM: &str = "consoles/jam.data.donder";
const SETUP: &str = "setups/main.data.donder";

#[test]
fn starter_keyboard_and_jam_console_load_typed() {
    let session = common::load_project(&common::starter_root());
    let project = &session.project;
    let setup = project
        .get::<donder_model::Setup>(*project.root().setup.id())
        .unwrap();
    let [device] = setup.input_devices.as_slice() else {
        panic!("the starter setup lists one input device");
    };
    let device = project
        .get::<donder_model::InputDevice>(*device.id())
        .unwrap();
    assert_eq!(device.controls.len(), 4);
    assert!(
        device
            .controls
            .iter()
            .all(|control| matches!(control.kind, InputControlKind::Button(_)))
    );
    let [console] = project.root().consoles.as_slice() else {
        panic!("the starter project lists one console");
    };
    let console = project.get::<donder_model::Console>(*console.id()).unwrap();
    assert_eq!(console.live_marks.len(), 1);
    assert_eq!(console.automations.len(), 1);
    assert_eq!(console.clips.len(), 3);
    assert_eq!(console.always, [console.clips[0].id]);
    assert!(console.inputs().all(|input| input.device == device.id));
    let events = console
        .events
        .iter()
        .map(|on| (&on.event, on.actions.as_slice()))
        .collect::<Vec<_>>();
    let (flash, wash) = (console.clips[1].id, console.clips[2].id);
    let decay = console.automations[0].id;
    assert!(matches!(
        events.as_slice(),
        [
            (InputEvent::Press(_), [Action::Play(play)]),
            (InputEvent::Press(_), [Action::Start { clip: started, .. }]),
            (InputEvent::Release(_), [Action::Stop { clip: stopped }]),
            (InputEvent::Press(_), [Action::Mark(_)]),
            (InputEvent::Press(_), [Action::Set(pressed)]),
            (InputEvent::Release(_), [Action::Set(released)]),
        ] if play.clip == flash
            && play.copies == 4
            && matches!(play.length, PlayLength::Fixed(_))
            && *started == wash
            && *stopped == wash
            && *pressed == decay
            && *released == decay
    ));
    donder_model::validate_project(project).unwrap();
}

#[test]
fn starter_keyboard_and_jam_console_save_byte_exactly() {
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
        let report = check_project(&root, &overrides, &mut Default::default());
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
            "Button { name: tap,",
            "Button { name: kick, description: none, input: Key { code: KeyQ } },
    Button { name: tap,",
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
fn invalid_consoles_are_rejected_at_the_reference() {
    assert_rejected(&[
        (
            JAM,
            "control: devices.keyboard.tap }",
            "control: devices.keyboard }",
            "`devices.keyboard` is an input device; an input is one of its controls",
        ),
        (
            JAM,
            "control: devices.keyboard.tap }",
            "control: devices.keyboard.snare }",
            "`devices.keyboard` has no control `snare`",
        ),
        (
            JAM,
            "copies: 4",
            "copies: 17",
            "plays 17 copies; `Play` plays 1 to 16 at once",
        ),
        (JAM, "copies: 4", "copies: 0", "plays 0 copies"),
        (
            JAM,
            "duration: 0.4s",
            "duration: 0s",
            "a length must be positive",
        ),
        (
            JAM,
            "param: decay_seconds",
            "param: accent",
            "`accent` cannot be automated",
        ),
        (
            JAM,
            "param: decay_seconds",
            "param: speed",
            "no parameter `speed`",
        ),
        (
            JAM,
            "Set { into: decay }",
            "Set { into: speed }",
            "no live automation `speed`",
        ),
        (
            JAM,
            "Stop { clip: wash }",
            "Stop { clip: burst }",
            "no clip `burst`",
        ),
        (
            JAM,
            "Mark { into: beat }",
            "Mark { into: downbeat }",
            "no live mark collection `downbeat`",
        ),
        (
            JAM,
            "marks: [LiveMarks { name: beat,",
            "marks: [LiveMarks { name: beat, description: none, color: #22c55e }, LiveMarks { name: beat,",
            "live mark collection `beat` is declared twice",
        ),
        (
            JAM,
            "always: [background],",
            "always: [flash],",
            "clip `flash` is played by `Play` and also held",
        ),
        (
            SETUP,
            "input_devices: [devices.keyboard],",
            "input_devices: [],",
            "the input device is not in the project's setup",
        ),
    ]);
}

#[test]
fn console_edits_are_validated_like_loading() {
    let mut session = common::load_project(&common::starter_root());
    let console = session
        .project
        .reusable::<donder_model::Console>()
        .values()
        .next()
        .unwrap()
        .clone();
    let rejected = |edit: &dyn Fn(&mut donder_model::Console),
                    expected: &dyn Fn(&ConsoleValidationError) -> bool| {
        let mut value = console.clone();
        edit(&mut value);
        let error = session.project.clone().put(value).unwrap_err();
        assert!(
            matches!(&error, EditError::Invalid(ProjectValidationError { problem: ValidationProblem::Console(problem), .. }) if expected(problem)),
            "{error:?}"
        );
    };
    let play = |console: &mut donder_model::Console| -> donder_model::Play {
        let Action::Play(play) = &mut console.events[0].actions[0] else {
            unreachable!("`kick` plays `flash`");
        };
        play.clone()
    };
    rejected(
        &|console| {
            let mut changed = play(console);
            changed.copies = 0;
            console.events[0].actions[0] = Action::Play(changed);
        },
        &|problem| {
            matches!(
                problem,
                ConsoleValidationError::Event {
                    problem: EventProblem::Copies(0),
                    ..
                }
            )
        },
    );
    rejected(
        &|console| {
            let mut changed = play(console);
            changed.length =
                PlayLength::Fixed(donder_language::DonderDuration(std::time::Duration::ZERO));
            console.events[0].actions[0] = Action::Play(changed);
        },
        &|problem| {
            matches!(
                problem,
                ConsoleValidationError::Event {
                    problem: EventProblem::NonPositiveDuration,
                    ..
                }
            )
        },
    );
    rejected(
        &|console| {
            let AutomationTarget::ClipParam { param, .. } = &mut console.automations[0].bindings[0]
            else {
                unreachable!("the starter's automation sets a clip parameter")
            };
            *param = donder_language::object_name("accent");
        },
        &|problem| {
            matches!(
                problem,
                ConsoleValidationError::Automation {
                    problem: LiveAutomationProblem::NotAutomatable(_),
                    ..
                }
            )
        },
    );
    rejected(
        &|console| {
            console.automations[0].bindings[0] = AutomationTarget::CompositionNodeParam {
                node_id: CompositionGraphNodeId(99),
                param: donder_language::object_name("decay_seconds"),
            };
        },
        &|problem| {
            matches!(
                problem,
                ConsoleValidationError::Automation {
                    problem: LiveAutomationProblem::MissingNode,
                    ..
                }
            )
        },
    );
    rejected(
        &|console| console.clips[2].name = console.clips[0].name.clone(),
        &|problem| {
            matches!(
                problem,
                ConsoleValidationError::Name(NameProblem::Duplicate(Member::ConsoleClip(_), _))
            )
        },
    );
    // A console of the project reads only the input devices of the project's
    // setup.
    let setup_id = *session.project.root().setup.id();
    let mut setup = session
        .project
        .get::<donder_model::Setup>(setup_id)
        .unwrap()
        .clone();
    setup.input_devices.clear();
    let error = session.project.put(setup).unwrap_err();
    assert!(
        matches!(
            error,
            EditError::Invalid(ProjectValidationError {
                problem: ValidationProblem::Console(ConsoleValidationError::Event {
                    problem: EventProblem::Input(InputProblem::NotInSetup),
                    ..
                }),
                ..
            })
        ),
        "{error:?}"
    );
}
