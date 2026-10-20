//! Input devices: the hardware rigs play from.
use donder_model::{
    ButtonInput, ChordQuality, DialInput, Heard, InputControl, InputControlId, InputControlKind,
    InputDevice, InputDeviceId, InputDriver, KeyCode, KeyRange, MAX_MIDI_CONTROL_CHANGE,
    MAX_MIDI_NOTE, Measure, Mode,
};
use donder_project_io::{ProjectSession, SourceObjectKind};

use super::{GuiMutationError, ResolvedGuiObject, blocked};
use donder_sequence_api::{
    GuiChordQuality, GuiControlInput, GuiDocument, GuiHeard, GuiInputControl, GuiInputControlKind,
    GuiInputDriver, GuiInputLimits, GuiKeyMode, GuiMeasure, InputDeviceGuiDocument,
    SetupInputDevice,
};

/// The highest MIDI inputs a control can read.
pub(super) fn input_limits() -> GuiInputLimits {
    GuiInputLimits {
        max_note: MAX_MIDI_NOTE,
        max_control_change: MAX_MIDI_CONTROL_CHANGE,
    }
}

/// Every key a control can read, in the language's order.
pub(super) fn key_codes() -> Vec<String> {
    KeyCode::ALL
        .iter()
        .map(|code| code.name().to_string())
        .collect()
}

pub(super) fn project_input_device(
    session: &ProjectSession,
    id: &InputDeviceId,
    device: &InputDevice,
) -> SetupInputDevice {
    SetupInputDevice {
        name: id.0.declared_name().to_string(),
        source_ref: super::patch::object_ref(&id.0, SourceObjectKind::InputDevice),
        read_only: !session.source.is_project_owned(id.0.document_id()),
        driver: match &device.driver {
            InputDriver::Keyboard => GuiInputDriver::Keyboard,
            InputDriver::Midi { port } => GuiInputDriver::Midi { port: port.clone() },
            InputDriver::Listen { port } => GuiInputDriver::Listen { port: port.clone() },
        },
        controls: device.controls.iter().map(gui_control).collect(),
    }
}

fn gui_control(control: &InputControl) -> GuiInputControl {
    let (kind, input) = match control.kind {
        InputControlKind::Button(input) => (
            GuiInputControlKind::Button,
            match input {
                ButtonInput::Key(code) => GuiControlInput::Key {
                    code: code.name().to_string(),
                },
                ButtonInput::Note(note) => GuiControlInput::Note { note },
                ButtonInput::ControlChange(number) => GuiControlInput::ControlChange { number },
                ButtonInput::Heard(heard) => GuiControlInput::Heard {
                    heard: gui_heard(heard),
                },
            },
        ),
        InputControlKind::Dial(input) => (
            GuiInputControlKind::Dial,
            match input {
                DialInput::ControlChange(number) => GuiControlInput::ControlChange { number },
                DialInput::PitchBend => GuiControlInput::PitchBend,
                DialInput::Measured(measure) => GuiControlInput::Measured {
                    measure: match measure {
                        Measure::KeyColor => GuiMeasure::KeyColor,
                        Measure::ChordColor => GuiMeasure::ChordColor,
                        Measure::Tension => GuiMeasure::Tension,
                        Measure::Loudness => GuiMeasure::Loudness,
                        Measure::Density => GuiMeasure::Density,
                        Measure::Register => GuiMeasure::Register,
                        Measure::Tempo => GuiMeasure::Tempo,
                    },
                },
            },
        ),
        InputControlKind::Keys(range) => (
            GuiInputControlKind::Keys,
            GuiControlInput::Notes {
                low: range.low,
                high: range.high,
            },
        ),
    };
    GuiInputControl {
        id: control.id.0,
        name: control.name.as_str().to_string(),
        description: control.description.clone(),
        kind,
        input,
    }
}

/// The control a GUI control describes, or why it describes none.
fn domain_control_kind(
    kind: GuiInputControlKind,
    input: &GuiControlInput,
) -> Result<InputControlKind, GuiMutationError> {
    Ok(match (kind, input) {
        (GuiInputControlKind::Button, GuiControlInput::Key { code }) => {
            InputControlKind::Button(ButtonInput::Key(
                KeyCode::from_name(code)
                    .ok_or_else(|| GuiMutationError::Invalid(format!("`{code}` is not a key.")))?,
            ))
        }
        (GuiInputControlKind::Button, GuiControlInput::Note { note }) => {
            InputControlKind::Button(ButtonInput::Note(*note))
        }
        (GuiInputControlKind::Button, GuiControlInput::ControlChange { number }) => {
            InputControlKind::Button(ButtonInput::ControlChange(*number))
        }
        (GuiInputControlKind::Dial, GuiControlInput::ControlChange { number }) => {
            InputControlKind::Dial(DialInput::ControlChange(*number))
        }
        (GuiInputControlKind::Dial, GuiControlInput::PitchBend) => {
            InputControlKind::Dial(DialInput::PitchBend)
        }
        (GuiInputControlKind::Keys, GuiControlInput::Notes { low, high }) => {
            InputControlKind::Keys(KeyRange {
                low: *low,
                high: *high,
            })
        }
        (GuiInputControlKind::Keys, _) | (_, GuiControlInput::Notes { .. }) => {
            return Err(GuiMutationError::Invalid(
                "Keys read a range of notes, and only keys do.".into(),
            ));
        }
        (GuiInputControlKind::Button, GuiControlInput::Heard { heard }) => {
            InputControlKind::Button(ButtonInput::Heard(domain_heard(*heard)))
        }
        (GuiInputControlKind::Dial, GuiControlInput::Measured { measure }) => {
            InputControlKind::Dial(DialInput::Measured(match measure {
                GuiMeasure::KeyColor => Measure::KeyColor,
                GuiMeasure::ChordColor => Measure::ChordColor,
                GuiMeasure::Tension => Measure::Tension,
                GuiMeasure::Loudness => Measure::Loudness,
                GuiMeasure::Density => Measure::Density,
                GuiMeasure::Register => Measure::Register,
                GuiMeasure::Tempo => Measure::Tempo,
            }))
        }
        (GuiInputControlKind::Button, GuiControlInput::Measured { .. }) => {
            return Err(GuiMutationError::Invalid(
                "A measure is a dial, not a button.".into(),
            ));
        }
        (GuiInputControlKind::Dial, GuiControlInput::Heard { .. }) => {
            return Err(GuiMutationError::Invalid(
                "A moment is a button, not a dial.".into(),
            ));
        }
        (GuiInputControlKind::Button, GuiControlInput::PitchBend) => {
            return Err(GuiMutationError::Invalid(
                "Pitch bend is a dial, not a button.".into(),
            ));
        }
        (GuiInputControlKind::Dial, GuiControlInput::Key { .. } | GuiControlInput::Note { .. }) => {
            return Err(GuiMutationError::Invalid(
                "Keys and notes are buttons, not dials.".into(),
            ));
        }
    })
}

pub(super) fn project_document(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let id = InputDeviceId(resolved.object_identity());
    let Some(device) = session.project.input_device(&id) else {
        return blocked("Input device was not found.", Vec::new());
    };
    GuiDocument::InputDevice {
        document: InputDeviceGuiDocument {
            description: device.description.clone(),
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            device: project_input_device(session, &id, device),
            key_codes: key_codes(),
            input_limits: input_limits(),
        },
    }
}

pub(super) fn edit(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    driver: GuiInputDriver,
    controls: Vec<GuiInputControl>,
) -> Result<(), GuiMutationError> {
    let id = InputDeviceId(resolved.object_identity());
    let description = session
        .project
        .input_device(&id)
        .ok_or_else(|| GuiMutationError::Invalid("Input device was not found.".into()))?
        .description
        .clone();
    let device = domain_input_device(id, description, driver, controls)?;
    session
        .project
        .replace_input_device(&device.id.clone(), device)
        .map_err(GuiMutationError::Invalid)
}

pub(super) fn domain_input_device(
    id: InputDeviceId,
    description: Option<String>,
    driver: GuiInputDriver,
    controls: Vec<GuiInputControl>,
) -> Result<InputDevice, GuiMutationError> {
    let controls = controls
        .into_iter()
        .map(|control| {
            let kind = domain_control_kind(control.kind, &control.input)?;
            Ok(InputControl {
                id: InputControlId(control.id),
                name: super::model::typed_name(&control.name)?,
                description: super::description::normalized(control.description),
                kind,
            })
        })
        .collect::<Result<_, GuiMutationError>>()?;
    let device = InputDevice {
        id,
        description,
        driver: match driver {
            GuiInputDriver::Keyboard => InputDriver::Keyboard,
            GuiInputDriver::Midi { port } => InputDriver::Midi {
                port: port
                    .map(|port| port.trim().to_string())
                    .filter(|port| !port.is_empty()),
            },
            GuiInputDriver::Listen { port } => InputDriver::Listen {
                port: port.filter(|port| !port.trim().is_empty()),
            },
        },
        controls,
    };
    device
        .validate()
        .map_err(|error| GuiMutationError::Invalid(format!("Invalid input device: {error}")))?;
    Ok(device)
}

fn gui_heard(heard: Heard) -> GuiHeard {
    match heard {
        Heard::ChordChange => GuiHeard::ChordChange,
        Heard::KeyChange => GuiHeard::KeyChange,
        Heard::Resolution => GuiHeard::Resolution,
        Heard::Beat => GuiHeard::Beat,
        Heard::RunUp => GuiHeard::RunUp,
        Heard::RunDown => GuiHeard::RunDown,
        Heard::BigChord => GuiHeard::BigChord,
        Heard::Chord(quality) => GuiHeard::Chord {
            quality: match quality {
                ChordQuality::Major => GuiChordQuality::Major,
                ChordQuality::Minor => GuiChordQuality::Minor,
                ChordQuality::Dominant => GuiChordQuality::Dominant,
                ChordQuality::Diminished => GuiChordQuality::Diminished,
                ChordQuality::Augmented => GuiChordQuality::Augmented,
                ChordQuality::Suspended => GuiChordQuality::Suspended,
            },
        },
        Heard::Mode(mode) => GuiHeard::Mode {
            mode: match mode {
                Mode::Major => GuiKeyMode::Major,
                Mode::Minor => GuiKeyMode::Minor,
            },
        },
        Heard::Idle { seconds } => GuiHeard::Idle { seconds },
    }
}

fn domain_heard(heard: GuiHeard) -> Heard {
    match heard {
        GuiHeard::ChordChange => Heard::ChordChange,
        GuiHeard::KeyChange => Heard::KeyChange,
        GuiHeard::Resolution => Heard::Resolution,
        GuiHeard::Beat => Heard::Beat,
        GuiHeard::RunUp => Heard::RunUp,
        GuiHeard::RunDown => Heard::RunDown,
        GuiHeard::BigChord => Heard::BigChord,
        GuiHeard::Chord { quality } => Heard::Chord(match quality {
            GuiChordQuality::Major => ChordQuality::Major,
            GuiChordQuality::Minor => ChordQuality::Minor,
            GuiChordQuality::Dominant => ChordQuality::Dominant,
            GuiChordQuality::Diminished => ChordQuality::Diminished,
            GuiChordQuality::Augmented => ChordQuality::Augmented,
            GuiChordQuality::Suspended => ChordQuality::Suspended,
        }),
        GuiHeard::Mode { mode } => Heard::Mode(match mode {
            GuiKeyMode::Major => Mode::Major,
            GuiKeyMode::Minor => Mode::Minor,
        }),
        GuiHeard::Idle { seconds } => Heard::Idle { seconds },
    }
}
