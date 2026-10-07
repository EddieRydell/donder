use donder_language::controller::{
    ArtNetConfig, ArtNetMode, Controller, ControllerId, ControllerPort, ControllerPortAddress,
    ControllerPortId, ControllerProtocol, DonderConfig, DonderDeviceId, E131Config, E131Mode,
};
use donder_project_io::{ProjectSession, SourceObjectKind};

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
        label: match &controller.protocol {
            ControllerProtocol::E131(config) => config.source_name.clone(),
            ControllerProtocol::ArtNet(config) => format!("Art-Net {}", config.destination),
            ControllerProtocol::Donder(config) => format!("Donder {}", config.device.as_str()),
        },
        source_ref: super::patch::object_ref(&id.0, SourceObjectKind::Controller),
        read_only: !session.source.is_project_owned(id.0.document_id()),
        config: (&controller.protocol).into(),
        ports: controller
            .ports
            .iter()
            .map(|port| SetupControllerPort {
                id: port.id.0,
                name: port.name.as_str().to_string(),
                address: match port.address {
                    ControllerPortAddress::E131Universe(address)
                    | ControllerPortAddress::ArtNetPort(address) => address,
                    ControllerPortAddress::DonderOutput(output) => u16::from(output),
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
    let id = ControllerId(resolved.object_identity());
    let Some(controller) = session.project.controller(&id) else {
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
    let id = ControllerId(resolved.object_identity());
    let description = session
        .project
        .controller(&id)
        .and_then(|controller| controller.description.clone());
    let controller = domain_controller(id, description, config, ports)?;
    session
        .project
        .replace_controller(&controller.id.clone(), controller)
        .map_err(GuiMutationError::Invalid)
}

pub(super) fn domain_controller(
    id: ControllerId,
    description: Option<String>,
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
        SetupControllerConfig::Donder { device } => ControllerProtocol::Donder(DonderConfig {
            device: DonderDeviceId::parse(&device).ok_or_else(|| {
                GuiMutationError::Invalid("Donder device must be 12 lowercase hex digits.".into())
            })?,
        }),
    };
    if ports.is_empty() {
        return Err(GuiMutationError::Invalid(
            "Add at least one output port.".into(),
        ));
    }
    let ports = ports
        .into_iter()
        .map(|port| {
            Ok(ControllerPort {
                id: ControllerPortId(port.id),
                name: super::model::typed_name(&port.name)?,
                slot_count: port.slot_count,
                address: match protocol {
                    ControllerProtocol::E131(_) => {
                        ControllerPortAddress::E131Universe(port.address)
                    }
                    ControllerProtocol::ArtNet(_) => {
                        ControllerPortAddress::ArtNetPort(port.address)
                    }
                    ControllerProtocol::Donder(_) => {
                        ControllerPortAddress::DonderOutput(u8::try_from(port.address).map_err(
                            |_| GuiMutationError::Invalid("Donder output must be 1-255.".into()),
                        )?)
                    }
                },
            })
        })
        .collect::<Result<_, GuiMutationError>>()?;
    let controller = Controller {
        id,
        description,
        protocol,
        ports,
    };
    controller.validate().map_err(|error| {
        GuiMutationError::Invalid(format!("Invalid controller configuration: {error:?}"))
    })?;
    Ok(controller)
}
