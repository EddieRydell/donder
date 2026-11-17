use donder_model::{
    ArtNetConfig, ArtNetMode, Controller, ControllerId, ControllerPort, ControllerPortId,
    ControllerProtocol, DonderConfig, DonderDeviceId, E131Config, E131Mode,
};
use donder_project_io::ProjectSession;

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
        label: super::model::object_address(session, *id)
            .declared_name()
            .to_string(),
        source_ref: super::model::object_ref(session, *id),
        config: (&controller.protocol).into(),
        ports: controller
            .ports()
            .iter()
            .map(|port| SetupControllerPort {
                id: port.id.0,
                name: port.name.as_str().to_string(),
                output: port.output,
                slot_count: port.slot_count,
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

/// `ports` are a Donder controller's outputs; E1.31 and Art-Net
/// controllers send universes and have none.
pub(super) fn domain_controller(
    id: ControllerId,
    description: Option<String>,
    config: SetupControllerConfig,
    ports: Vec<SetupControllerPort>,
) -> Result<Controller, GuiMutationError> {
    use crate::dto::{GuiField, GuiMember, GuiProblemSite};
    let invalid_address = |field: GuiField| {
        move |_| {
            GuiMutationError::At(
                GuiProblemSite::object(field),
                "Not a network address.".into(),
            )
        }
    };
    if !ports.is_empty() && !matches!(config, SetupControllerConfig::Donder { .. }) {
        return Err(GuiMutationError::Invalid(
            "An E1.31 or Art-Net controller sends universes, not outputs.".into(),
        ));
    }
    let protocol = match config {
        SetupControllerConfig::E131 {
            sender_name,
            bind_address,
            priority,
            destination,
            universes,
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
        }),
        SetupControllerConfig::ArtNet {
            bind_address,
            destination,
            broadcast,
            universes,
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
        }),
        SetupControllerConfig::Donder { device } => ControllerProtocol::Donder(DonderConfig {
            device: DonderDeviceId::parse(&device).ok_or_else(|| {
                GuiMutationError::At(
                    GuiProblemSite::object(GuiField::Device),
                    "Choose a Donder controller.".into(),
                )
            })?,
            ports: ports
                .into_iter()
                .map(|port| {
                    Ok(ControllerPort {
                        id: ControllerPortId(port.id),
                        name: super::model::typed_name(&port.name).map_err(|error| {
                            error.at(GuiProblemSite::of(
                                GuiMember::Port { id: port.id },
                                Some(GuiField::Name),
                            ))
                        })?,
                        output: port.output,
                        slot_count: port.slot_count,
                    })
                })
                .collect::<Result<_, GuiMutationError>>()?,
        }),
    };
    Ok(Controller {
        id,
        description,
        protocol,
    })
}
