use dawn_language::controller::{
    ArtNetConfig, ArtNetMode, Controller, ControllerId, ControllerPort, ControllerPortAddress,
    ControllerPortId, ControllerProtocol, E131Config, E131Mode,
};
use dawn_project_io::{ProjectSession, SourceObjectKind};

use super::{GuiMutationError, ResolvedGuiObject, blocked};
use crate::dto::{
    ControllerGuiDocument, GuiDocument, SetupController, SetupControllerConfig, SetupControllerPort,
};

pub(super) fn project_controller(
    session: &ProjectSession,
    id: &ControllerId,
    controller: &Controller,
) -> SetupController {
    SetupController {
        label: super::setup::source_key(&id.0),
        source_ref: ResolvedGuiObject {
            identity: id.0.clone(),
            kind: SourceObjectKind::Controller,
        }
        .source_ref(),
        read_only: !session.source.is_project_owned(id.0.document_id()),
        config: (&controller.protocol).into(),
        ports: controller
            .ports
            .iter()
            .map(|port| SetupControllerPort {
                id: port.id.0,
                address: match port.address {
                    ControllerPortAddress::E131Universe(address)
                    | ControllerPortAddress::ArtNetPort(address) => address,
                },
                slot_count: port.slot_count,
            })
            .collect(),
    }
}

pub(super) fn project_document(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let id = ControllerId(resolved.identity.clone());
    let Some(controller) = session.project.controllers.get(&id) else {
        return blocked("Controller was not found.", Vec::new());
    };
    GuiDocument::Controller {
        document: ControllerGuiDocument {
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
    let controller = domain_controller(config, ports)?;
    let target = session
        .project
        .controllers
        .get_mut(&ControllerId(resolved.identity.clone()))
        .ok_or_else(|| GuiMutationError::Invalid("Controller was not found.".into()))?;
    *target = controller;
    Ok(())
}

pub(super) fn domain_controller(
    config: SetupControllerConfig,
    ports: Vec<SetupControllerPort>,
) -> Result<Controller, GuiMutationError> {
    let invalid_address =
        |error| GuiMutationError::Invalid(format!("Invalid network address: {error}"));
    let protocol = match config {
        SetupControllerConfig::E131 {
            source_name,
            bind_address,
            priority,
            destination,
        } => ControllerProtocol::E131(E131Config {
            source_name,
            bind_address: bind_address.parse().map_err(invalid_address)?,
            priority,
            mode: match destination {
                Some(destination) => E131Mode::Unicast {
                    destination: destination.parse().map_err(invalid_address)?,
                },
                None => E131Mode::Multicast,
            },
        }),
        SetupControllerConfig::ArtNet {
            bind_address,
            destination,
            broadcast,
        } => ControllerProtocol::ArtNet(ArtNetConfig {
            bind_address: bind_address.parse().map_err(invalid_address)?,
            destination: destination.parse().map_err(invalid_address)?,
            mode: if broadcast {
                ArtNetMode::Broadcast
            } else {
                ArtNetMode::Unicast
            },
        }),
    };
    if ports.is_empty() {
        return Err(GuiMutationError::Invalid(
            "Add at least one output port.".into(),
        ));
    }
    let ports = ports
        .into_iter()
        .map(|port| ControllerPort {
            id: ControllerPortId(port.id),
            slot_count: port.slot_count,
            address: match protocol {
                ControllerProtocol::E131(_) => ControllerPortAddress::E131Universe(port.address),
                ControllerProtocol::ArtNet(_) => ControllerPortAddress::ArtNetPort(port.address),
            },
        })
        .collect();
    let controller = Controller { protocol, ports };
    controller.validate().map_err(|error| {
        GuiMutationError::Invalid(format!("Invalid controller configuration: {error:?}"))
    })?;
    Ok(controller)
}
