//! Input devices: the hardware rigs play from.
use donder_model::{
    ButtonInput, InputControl, InputControlId, InputControlKind, InputDevice, InputDeviceId,
    InputDriver, KeyCode,
};
use donder_project_io::{ProjectSession, SourceObjectKind};

use super::{GuiMutationError, ResolvedGuiObject, blocked};
use donder_sequence_api::{
    GuiButtonInput, GuiDocument, GuiInputControl, GuiInputDriver, InputDeviceGuiDocument,
    SetupInputDevice,
};

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
        driver: match device.driver {
            InputDriver::Keyboard => GuiInputDriver::Keyboard,
        },
        controls: device
            .controls
            .iter()
            .map(|control| GuiInputControl {
                id: control.id.0,
                name: control.name.as_str().to_string(),
                description: control.description.clone(),
                input: match control.kind {
                    InputControlKind::Button(ButtonInput::Key(code)) => GuiButtonInput::Key {
                        code: code.name().to_string(),
                    },
                },
            })
            .collect(),
    }
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
            let GuiButtonInput::Key { code } = &control.input;
            let code = KeyCode::from_name(code)
                .ok_or_else(|| GuiMutationError::Invalid(format!("`{code}` is not a key.")))?;
            Ok(InputControl {
                id: InputControlId(control.id),
                name: super::model::typed_name(&control.name)?,
                description: super::description::normalized(control.description),
                kind: InputControlKind::Button(ButtonInput::Key(code)),
            })
        })
        .collect::<Result<_, GuiMutationError>>()?;
    let device = InputDevice {
        id,
        description,
        driver: match driver {
            GuiInputDriver::Keyboard => InputDriver::Keyboard,
        },
        controls,
    };
    device
        .validate()
        .map_err(|error| GuiMutationError::Invalid(format!("Invalid input device: {error}")))?;
    Ok(device)
}
