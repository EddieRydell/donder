//! Input devices: the hardware consoles play from.
use donder_model::{
    AxisInput, ButtonInput, InputControl, InputControlId, InputControlKind, InputDevice,
    InputDeviceId, InputDriver, KeyCode, MAX_MIDI_CHANNEL, MAX_MIDI_CONTROL_CHANGE, MAX_MIDI_NOTE,
    MidiChannel, NoteRange, OscAddress,
};
use donder_project_io::ProjectSession;

use super::{GuiMutationError, ResolvedGuiObject, blocked};
use crate::dto::{
    GuiControlInput, GuiDocument, GuiInputControl, GuiInputControlKind, GuiInputDriver,
    GuiInputLimits, InputDeviceGuiDocument, SetupInputDevice,
};

/// The highest MIDI inputs a control can read.
pub(super) fn input_limits() -> GuiInputLimits {
    GuiInputLimits {
        max_note: MAX_MIDI_NOTE,
        max_control_change: MAX_MIDI_CONTROL_CHANGE,
        max_channel: MAX_MIDI_CHANNEL,
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
        name: super::model::object_address(session, *id)
            .declared_name()
            .to_string(),
        source_ref: super::model::object_ref(session, *id),
        driver: match &device.driver {
            InputDriver::Keyboard => GuiInputDriver::Keyboard,
            InputDriver::Midi { port } => GuiInputDriver::Midi { port: port.clone() },
            InputDriver::Osc { port } => GuiInputDriver::Osc { port: *port },
        },
        controls: device.controls.iter().map(gui_control).collect(),
    }
}

fn gui_control(control: &InputControl) -> GuiInputControl {
    let (kind, input) = match &control.kind {
        InputControlKind::Button(input) => (
            GuiInputControlKind::Button,
            match input {
                ButtonInput::Key(code) => GuiControlInput::Key {
                    code: code.name().to_string(),
                },
                ButtonInput::Notes { notes, channel } => GuiControlInput::Notes {
                    low: notes.low,
                    high: notes.high,
                    channel: channel.map(|channel| channel.0),
                },
                ButtonInput::ControlChange { number, channel } => GuiControlInput::ControlChange {
                    number: *number,
                    channel: channel.map(|channel| channel.0),
                },
                ButtonInput::Address(path) => GuiControlInput::Address {
                    path: path.0.clone(),
                },
            },
        ),
        InputControlKind::Axis(input) => (
            GuiInputControlKind::Axis,
            match input {
                AxisInput::ControlChange { number, channel } => GuiControlInput::ControlChange {
                    number: *number,
                    channel: channel.map(|channel| channel.0),
                },
                AxisInput::PitchBend { channel } => GuiControlInput::PitchBend {
                    channel: channel.map(|channel| channel.0),
                },
                AxisInput::Address { path, low, high } => GuiControlInput::AddressRange {
                    path: path.0.clone(),
                    low: *low,
                    high: *high,
                },
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
        (GuiInputControlKind::Button, GuiControlInput::Notes { low, high, channel }) => {
            InputControlKind::Button(ButtonInput::Notes {
                notes: NoteRange {
                    low: *low,
                    high: *high,
                },
                channel: channel.map(MidiChannel),
            })
        }
        (GuiInputControlKind::Button, GuiControlInput::ControlChange { number, channel }) => {
            InputControlKind::Button(ButtonInput::ControlChange {
                number: *number,
                channel: channel.map(MidiChannel),
            })
        }
        (GuiInputControlKind::Axis, GuiControlInput::ControlChange { number, channel }) => {
            InputControlKind::Axis(AxisInput::ControlChange {
                number: *number,
                channel: channel.map(MidiChannel),
            })
        }
        (GuiInputControlKind::Button, GuiControlInput::Address { path }) => {
            InputControlKind::Button(ButtonInput::Address(OscAddress(path.trim().to_string())))
        }
        (GuiInputControlKind::Axis, GuiControlInput::PitchBend { channel }) => {
            InputControlKind::Axis(AxisInput::PitchBend {
                channel: channel.map(MidiChannel),
            })
        }
        (GuiInputControlKind::Axis, GuiControlInput::AddressRange { path, low, high }) => {
            InputControlKind::Axis(AxisInput::Address {
                path: OscAddress(path.trim().to_string()),
                low: *low,
                high: *high,
            })
        }
        (GuiInputControlKind::Button, GuiControlInput::PitchBend { .. }) => {
            return Err(GuiMutationError::Invalid(
                "Pitch bend is an axis, not a button.".into(),
            ));
        }
        (
            GuiInputControlKind::Axis,
            GuiControlInput::Key { .. }
            | GuiControlInput::Notes { .. }
            | GuiControlInput::Address { .. },
        ) => {
            return Err(GuiMutationError::Invalid(
                "Keys, notes and addresses without a range are buttons, not axes.".into(),
            ));
        }
        (GuiInputControlKind::Button, GuiControlInput::AddressRange { .. }) => {
            return Err(GuiMutationError::Invalid(
                "An address with a range is an axis, not a button.".into(),
            ));
        }
    })
}

pub(super) fn project_document(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let id = resolved.id::<InputDeviceId>();
    let Some(device) = session.project.get::<donder_model::InputDevice>(id) else {
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
    let id = resolved.id::<InputDeviceId>();
    let description = session
        .project
        .get::<donder_model::InputDevice>(id)
        .ok_or_else(|| GuiMutationError::Invalid("Input device was not found.".into()))?
        .description
        .clone();
    let device = domain_input_device(id, description, driver, controls)?;
    session.project.put(device).map_err(GuiMutationError::from)
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
            GuiInputDriver::Osc { port } => InputDriver::Osc { port },
        },
        controls,
    };
    device
        .validate()
        .map_err(|error| GuiMutationError::Invalid(format!("Invalid input device: {error}")))?;
    Ok(device)
}
