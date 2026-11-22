use donder_model::{
    ArtNetConfig, ArtNetMode, Controller, ControllerId, ControllerPort, ControllerPortId,
    ControllerProtocol, DonderConfig, DonderDeviceId, DonderPort, E131Config, E131Mode,
};
use donder_project_io::ProjectSession;
use std::num::NonZeroU32;

use super::{GuiMutationError, ResolvedGuiObject, blocked};
use crate::dto::{
    ControllerGuiDocument, GuiDocument, SetupController, SetupControllerConfig,
    SetupControllerPort, SetupDonderOutput,
};

pub(super) fn project_controller(
    session: &ProjectSession,
    id: &ControllerId,
    controller: &Controller,
) -> SetupController {
    SetupController {
        label: super::model::object_address(session, *id)
            .declared_name()
            .to_string(),
        source_ref: super::model::object_ref(session, *id),
        config: (&controller.protocol).into(),
        ports: controller
            .ports()
            .into_iter()
            .map(|port| SetupControllerPort {
                id: port.id.0,
                name: port.name.clone(),
                description: port.description.clone(),
                donder: controller
                    .donder_ports()
                    .iter()
                    .find(|donder| donder.port.id == port.id)
                    .map(|donder| SetupDonderOutput {
                        output: donder.output,
                        slot_count: donder.slot_count,
                    }),
            })
            .collect(),
    }
}

pub(super) fn project_document(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let id = resolved.id::<ControllerId>();
    let Some(controller) = session.project.get::<donder_model::Controller>(id) else {
        return blocked("Controller was not found.", Vec::new());
    };
    GuiDocument::Controller {
        document: ControllerGuiDocument {
            description: controller.description.clone(),
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            controller: project_controller(session, &id, controller),
        },
    }
}

pub(super) fn edit(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    config: SetupControllerConfig,
    ports: Vec<SetupControllerPort>,
) -> Result<(), GuiMutationError> {
    let id = resolved.id::<ControllerId>();
    let description = session
        .project
        .get::<donder_model::Controller>(id)
        .and_then(|controller| controller.description.clone());
    let controller = domain_controller(id, description, config, ports)?;
    session
        .project
        .put(controller)
        .map_err(GuiMutationError::from)
}

/// `ports` are the controller's physical outputs; a Donder controller's
/// each name the output it drives, and other controllers' name none.
pub(super) fn domain_controller(
    id: ControllerId,
    description: Option<String>,
    config: SetupControllerConfig,
    ports: Vec<SetupControllerPort>,
) -> Result<Controller, GuiMutationError> {
    use crate::dto::{GuiField, GuiProblemSite};
    let invalid_address = |field: GuiField| {
        move |_| {
            GuiMutationError::At(
                GuiProblemSite::object(field),
                "Not a network address.".into(),
            )
        }
    };
    let nonzero_fseq_channel = |channel| {
        NonZeroU32::new(channel).ok_or_else(|| {
            GuiMutationError::At(
                GuiProblemSite::object(GuiField::FseqChannel),
                "FSEQ channels count from 1.".into(),
            )
        })
    };
    let donder = matches!(config, SetupControllerConfig::Donder { .. });
    if ports.iter().any(|port| port.donder.is_some() != donder) {
        return Err(GuiMutationError::Invalid(
            "Only a Donder controller's ports drive a Donder output.".into(),
        ));
    }
    let port = |port: &SetupControllerPort| ControllerPort {
        id: ControllerPortId(port.id),
        name: port.name.clone(),
        description: port
            .description
            .clone()
            .filter(|text| !text.trim().is_empty()),
    };
    let protocol = match config {
        SetupControllerConfig::E131 {
            sender_name,
            bind_address,
            priority,
            destination,
            universes,
            fseq_channel,
        } => ControllerProtocol::E131(E131Config {
            sender_name,
            bind_address: bind_address
                .parse()
                .map_err(invalid_address(GuiField::Interface))?,
            priority,
            mode: match destination {
                Some(destination) => E131Mode::Unicast {
                    destination: destination
                        .parse()
                        .map_err(invalid_address(GuiField::Destination))?,
                },
                None => E131Mode::Multicast,
            },
            universes: universes.into(),
            fseq_channel: fseq_channel.map(nonzero_fseq_channel).transpose()?,
            ports: ports.iter().map(port).collect(),
        }),
        SetupControllerConfig::ArtNet {
            bind_address,
            destination,
            broadcast,
            universes,
            fseq_channel,
        } => ControllerProtocol::ArtNet(ArtNetConfig {
            bind_address: bind_address
                .parse()
                .map_err(invalid_address(GuiField::Interface))?,
            destination: destination
                .parse()
                .map_err(invalid_address(GuiField::Destination))?,
            mode: if broadcast {
                ArtNetMode::Broadcast
            } else {
                ArtNetMode::Unicast
            },
            universes: universes.into(),
            fseq_channel: fseq_channel.map(nonzero_fseq_channel).transpose()?,
            ports: ports.iter().map(port).collect(),
        }),
        SetupControllerConfig::Donder { device } => ControllerProtocol::Donder(DonderConfig {
            device: DonderDeviceId::parse(&device).ok_or_else(|| {
                GuiMutationError::At(
                    GuiProblemSite::object(GuiField::Device),
                    "Choose a Donder controller.".into(),
                )
            })?,
            ports: ports
                .iter()
                .filter_map(|value| {
                    value.donder.map(|physical| DonderPort {
                        port: port(value),
                        output: physical.output,
                        slot_count: physical.slot_count,
                    })
                })
                .collect(),
        }),
    };
    Ok(Controller {
        id,
        description,
        protocol,
    })
}
