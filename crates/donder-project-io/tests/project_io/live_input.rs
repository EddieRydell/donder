//! Input devices and consoles: the starter's keyboard and jam console, and
//! the diagnostics for invalid ones.
use crate::common;

use camino::Utf8PathBuf;
use donder_model::{
    Action, AutomationTarget, CompositionGraphNodeId, ConsoleValidationError, EditError,
    EventProblem, InputControlKind, PlayLength, ProjectValidationError, ValidationProblem,
};
use donder_project_io::{check_project, project_source_texts};
use donder_runtime_types::InputHappening;

const KEYBOARD: &str = "input_devices/keyboard.data.donder";
const JAM: &str = "consoles/jam.data.donder";

#[test]
fn starter_keyboard_and_jam_console_load_typed() {
    let session = common::load_project(&common::starter_root());
    let project = session.project();
    let devices = project
        .objects::<donder_model::InputDevice>()
        .collect::<Vec<_>>();
    let [(device_id, device)] = devices.as_slice() else {
        panic!("the starter project has one input device");
    };
    assert_eq!(device.controls.len(), 4);
    assert!(
        device
            .controls
            .iter()
            .all(|control| matches!(control.kind, InputControlKind::Button(_)))
    );
    let consoles = project
        .objects::<donder_model::Console>()
        .map(|(_, console)| console)
        .collect::<Vec<_>>();
    let [console] = consoles.as_slice() else {
        panic!("the starter project has one console");
    };
    assert_eq!(console.composition.marks.len(), 1);
    assert_eq!(console.set_targets().len(), 1);
    assert_eq!(console.composition.clips.len(), 3);
    assert_eq!(console.always.len(), 1);
    assert_eq!(console.always[0].clip, console.composition.clips[0].id);
    assert!(console.inputs().all(|input| input.device == *device_id));
    let events = console
        .events
        .iter()
        .map(|on| (on.happening, on.actions.as_slice()))
        .collect::<Vec<_>>();
    let (flash, wash) = (
        console.composition.clips[1].id,
        console.composition.clips[2].id,
    );
    let decay = console.set_targets()[0].clone();
    assert!(matches!(
        events.as_slice(),
        [
            (InputHappening::Press, [Action::Play(play)]),
            (InputHappening::Press, [Action::Start { clip: started, .. }]),
            (InputHappening::Release, [Action::Stop { clip: stopped }]),
            (InputHappening::Press, [Action::Mark(_)]),
            (InputHappening::Press, [Action::Set { target: pressed, .. }]),
            (InputHappening::Release, [Action::Set { target: released, .. }]),
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
    for path in [KEYBOARD, JAM, donder_project_io::PROJECT_ROOT_FILE] {
        let path = Utf8PathBuf::from(path);
        assert_eq!(before[&path], after[&path], "{path}");
    }
    assert_eq!(session.project(), common::load_project(&root).project());
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
            "two controls are named kick",
        ),
        (
            KEYBOARD,
            "code: KeyS",
            "code: KeyA",
            "controls swell and kick both read key KeyA",
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
            "control: devices.keyboard.tap,",
            "control: devices.keyboard,",
            "`devices.keyboard` is an input device; an input is one of its controls",
        ),
        (
            JAM,
            "control: devices.keyboard.tap,",
            "control: devices.keyboard.snare,",
            "`devices.keyboard` has no control `snare`",
        ),
        (
            JAM,
            "copies: 4",
            "copies: 17",
            "17 copies is outside 1 to 16",
        ),
        (JAM, "copies: 4", "copies: 0", "0 copies is outside"),
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
            "parameter accent can't be set",
        ),
        (
            JAM,
            "param: decay_seconds",
            "param: speed",
            "no parameter speed",
        ),
        (
            JAM,
            "ClipParam { clip: background",
            "ClipParam { clip: burst",
            "no clip `burst`",
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
            "no mark collection `downbeat`",
        ),
        (
            JAM,
            "marks: [MarkCollection { name: beat,",
            "marks: [MarkCollection { name: beat, description: none, color: #22c55e, times: [] }, MarkCollection { name: beat,",
            "two mark collections are named beat",
        ),
        (
            JAM,
            "always: [Always { clip: background, length: 4s }],",
            "always: [Always { clip: flash, length: 4s }],",
            "clip flash is played by Play and held",
        ),
    ]);
}

#[test]
fn console_edits_are_validated_like_loading() {
    let mut session = common::load_project(&common::starter_root());
    let (console_id, console) = session
        .project()
        .objects::<donder_model::Console>()
        .next()
        .unwrap();
    let console = console.clone();
    let rejected = |edit: &dyn Fn(&mut donder_model::Console),
                    expected: &dyn Fn(&ConsoleValidationError) -> bool| {
        let mut value = console.clone();
        edit(&mut value);
        let error = session
            .project()
            .clone()
            .put((console_id, value))
            .unwrap_err();
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
            let Action::Set {
                target: AutomationTarget::ClipParam { param, .. },
                ..
            } = &mut console.events[4].actions[0]
            else {
                unreachable!("the starter's boost key sets a clip parameter")
            };
            *param = donder_language::object_name("accent");
        },
        &|problem| {
            matches!(
                problem,
                ConsoleValidationError::Event {
                    problem: EventProblem::NotAutomatable(_),
                    ..
                }
            )
        },
    );
    rejected(
        &|console| {
            console.events[4].actions[0] = Action::Set {
                target: AutomationTarget::CompositionNodeParam {
                    node_id: CompositionGraphNodeId(99),
                    param: donder_language::object_name("decay_seconds"),
                },
                window: donder_model::Window::FULL,
            };
        },
        &|problem| {
            matches!(
                problem,
                ConsoleValidationError::Event {
                    problem: EventProblem::MissingNode,
                    ..
                }
            )
        },
    );
    // Removing an input device removes the events that read it.
    let devices = session
        .project()
        .reusable::<donder_model::InputDevice>()
        .keys()
        .map(|&id| id.into())
        .collect::<Vec<donder_model::ObjectId>>();
    session
        .edit(|edit| {
            for id in devices {
                edit.remove_object(id)?;
            }
            Ok::<_, donder_project_io::SessionEditError>(())
        })
        .unwrap();
    let repairs = session.take_repairs();
    assert!(
        !repairs.is_empty()
            && repairs
                .iter()
                .all(|repair| repair.action == donder_model::GiveWay::Remove
                    && matches!(repair.site.member, Some(donder_model::Member::Event(_)))),
        "{repairs:?}"
    );
}
