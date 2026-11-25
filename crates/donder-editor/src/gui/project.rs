use donder_model::{ConsoleId, ControllerId, InputDeviceId, SequenceId};
use donder_project_io::ProjectSession;

use super::model::object_ref;
use super::{GuiMutationError, ResolvedGuiObject};
use crate::dto::{GuiDocument, ProjectGuiDocument, ProjectGuiEdit};

/// A new sequence's or console's frame rate: Perform's target rate.
const NEW_FRAME_RATE: u32 = 100;
/// A new sequence's length.
const NEW_SEQUENCE_SECONDS: f64 = 60.0;

pub(super) fn edit_project(
    session: &mut ProjectSession,
    edit: ProjectGuiEdit,
) -> Result<(), GuiMutationError> {
    use super::ownership::add_in_own_file;
    match edit {
        ProjectGuiEdit::AddSequence {
            initial_color,
            name,
        } => {
            let color = super::model::parse_color(&initial_color)?;
            let duration = super::checked_gui_duration(NEW_SEQUENCE_SECONDS)?;
            let id = SequenceId(session.new_id());
            let sequence = new_sequence(duration, color);
            add_in_own_file(session, id, sequence, &name)
        }
        ProjectGuiEdit::AddConsole {
            initial_color,
            name,
        } => {
            let color = super::model::parse_color(&initial_color)?;
            let id = ConsoleId(session.new_id());
            let console = donder_model::Console {
                description: None,
                frame_rate: NEW_FRAME_RATE,
                composition: donder_model::Composition::default(color),
                always: Vec::new(),
                events: Vec::new(),
            };
            add_in_own_file(session, id, console, &name)
        }
        ProjectGuiEdit::AddInputDevice {
            driver,
            controls,
            name,
        } => {
            let id = InputDeviceId(session.new_id());
            let device = super::input_device::domain_input_device(None, driver, controls)?;
            add_in_own_file(session, id, device, &name)
        }
        ProjectGuiEdit::AddController {
            config,
            ports,
            name,
        } => {
            let id = ControllerId(session.new_id());
            let controller = super::controller::domain_controller(None, config, ports)?;
            add_in_own_file(session, id, controller, &name)
        }
    }
}

pub(super) fn project_root(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let project = session.project();
    let root = project.root();
    let outputs = match super::patch::project_outputs(session) {
        Ok(outputs) => outputs,
        Err(reason) => return super::blocked(reason, Vec::new()),
    };
    GuiDocument::Project {
        document: ProjectGuiDocument {
            description: root.description.clone(),
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            sequences: project
                .reusable::<donder_model::Sequence>()
                .keys()
                .map(|id| object_ref(session, *id))
                .collect(),
            consoles: project
                .reusable::<donder_model::Console>()
                .keys()
                .map(|id| object_ref(session, *id))
                .collect(),
            new_sequence_name: super::model::new_object_name(
                session,
                donder_model::ObjectKind::Sequence,
            ),
            new_console_name: super::model::new_object_name(
                session,
                donder_model::ObjectKind::Console,
            ),
            layout_ref: object_ref(session, root.layout),
            outputs,
            controllers: project
                .controllers()
                .map(|controller| {
                    super::controller::project_controller(session, &controller.id(), &controller)
                })
                .collect(),
            input_devices: project
                .input_devices()
                .map(|device| {
                    super::input_device::project_input_device(session, &device.id(), &device)
                })
                .collect(),
            key_codes: super::input_device::key_codes(),
            input_limits: super::input_device::input_limits(),
            new_controller_name: super::model::new_object_name(
                session,
                donder_model::ObjectKind::Controller,
            ),
            new_input_device_name: super::model::new_object_name(
                session,
                donder_model::ObjectKind::InputDevice,
            ),
        },
    }
}

/// A new sequence: the default layer drawn to the output, and one empty
/// collection of marks.
fn new_sequence(
    duration: donder_language::DonderDuration,
    color: donder_runtime_types::Color,
) -> donder_model::Sequence {
    donder_model::Sequence {
        description: None,
        duration,
        frame_rate: NEW_FRAME_RATE,
        audio: donder_model::SequenceAudio::None,
        composition: donder_model::Composition {
            marks: vec![donder_model::MarkCollection {
                id: donder_model::MarkCollectionId(0),
                name: donder_runtime_types::Identifier::new("marks".to_string())
                    .unwrap_or_else(|_| unreachable!("literal names are valid")),
                description: None,
                display_color: color,
                marks: Vec::new(),
            }],
            ..donder_model::Composition::default(color)
        },
    }
}
