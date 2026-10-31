//! Vixen controllers as Donder controllers. An E1.31 controller's outputs
//! are numbered from 0, and each active universe sends `Size` slots holding
//! outputs `Start` on. Vixen: `E131OutputPlugin`, `UniverseEntry`.

use super::config::ConfigController;
use super::contract;
use crate::report::ControllerSkip;
use crate::setup::{ProfileController, ProfileOutput, Slot};
use crate::xml::Element;
use donder_model::{ControllerProtocol, E131Config, E131Mode};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr};

const E131: &str = "771d7eba-662a-48d5-aedb-445c8708e878";

/// The output modules Vixen ships, by type id, besides E1.31.
const OTHER_MODULES: [(&str, &str); 9] = [
    ("110b5fe9-cd4d-4e73-bb5b-67552a1ad6d1", "DDP"),
    ("8abf27a7-3156-4f4c-9d43-ada45cdd6d0c", "DMX USB Pro"),
    ("b6ee6308-189c-4268-8996-32a4bab8ab5f", "dummy lighting"),
    ("2565e617-89db-4761-be8d-4d5574cafba9", "Elexol EtherIO"),
    ("d22c9233-16e9-4008-a215-e577f78a7a2e", "generic serial"),
    ("1c4bfd64-3172-43a7-a7fc-0a77c88b0489", "launcher"),
    ("692b66b9-e831-4f39-bb73-7653fa62b053", "RDS"),
    ("f66130a6-ff8a-48a7-bf1e-fac6cec2fc2c", "Renard"),
    ("7911568a-8eda-4d1d-9e72-be41d7a843e4", "OpenDMX"),
];

/// One universe a controller sends: its first output and how many slots.
struct Universe {
    start: u32,
    slots: u32,
}

/// The show's controllers, and where each controller output's channel is.
pub(crate) struct Controllers {
    pub(crate) controllers: Vec<ProfileController>,
    /// By controller output id.
    pub(crate) slots: HashMap<String, Slot>,
}

/// The E1.31 settings of a controller, by its module instance.
fn protocol(
    controller: &ConfigController,
    data: Option<&Element>,
    disabled: bool,
) -> Result<(ControllerProtocol, Vec<(u16, Universe)>), ControllerSkip> {
    if controller.hardware_id != E131 {
        return Err(OTHER_MODULES
            .iter()
            .find(|(id, _)| *id == controller.hardware_id)
            .map_or_else(
                || ControllerSkip::UnknownModule(controller.hardware_id.clone()),
                |(_, name)| ControllerSkip::Protocol(name),
            ));
    }
    if disabled {
        return Err(ControllerSkip::Inactive);
    }
    let data = data.ok_or(ControllerSkip::UnknownModule(
        controller.hardware_id.clone(),
    ))?;
    if field(contract::boolean(data, "Blind", false))? {
        return Err(ControllerSkip::Blind);
    }
    let priority = field(contract::int(data, "Priority", 100))?;
    let priority = u8::try_from(priority)
        .ok()
        .filter(|priority| (1..=200).contains(priority))
        .ok_or(ControllerSkip::Priority(priority))?;
    let entries: Vec<&Element> = contract::field(data, "Universes")
        .map(|universes| universes.children("UniverseEntry").collect())
        .unwrap_or_default();
    // Vixen: E131OutputPlugin.Start takes the address from the first
    // universe when the controller has none, as older plugins kept it there.
    let mut unicast = contract::text(data, "Unicast").map(str::to_string);
    let mut multicast = contract::text(data, "Multicast").map(str::to_string);
    if unicast.is_none()
        && multicast.is_none()
        && let Some(first) = entries.first()
    {
        unicast = contract::text(first, "Unicast").map(str::to_string);
        multicast = contract::text(first, "Multicast").map(str::to_string);
    }
    let mode = match (unicast, multicast) {
        (Some(address), _) => E131Mode::Unicast {
            destination: address
                .trim()
                .parse::<IpAddr>()
                .map_err(|_| ControllerSkip::Address(address.clone()))?,
        },
        (None, Some(_)) => E131Mode::Multicast,
        (None, None) => return Err(ControllerSkip::Address(String::new())),
    };
    let outputs = controller.outputs.len() as u32;
    let mut universes = Vec::new();
    let mut numbers = HashSet::new();
    for entry in entries {
        // Vixen: E131OutputPlugin.UpdateState sends only active universes
        // that start inside the controller's outputs.
        let start = field(contract::int(entry, "Start", 0))?;
        if !field(contract::boolean(entry, "Active", true))?
            || u32::try_from(start).is_ok_and(|start| start >= outputs)
        {
            continue;
        }
        let number = field(contract::int(entry, "Universe", 1))?;
        let size = field(contract::int(entry, "Size", 512))?;
        let start = u32::try_from(start)
            .map_err(|_| ControllerSkip::Setting("Start".into(), start.to_string()))?;
        if !(1..=63_999).contains(&number) {
            return Err(ControllerSkip::Universe(number));
        }
        if !numbers.insert(number) {
            return Err(ControllerSkip::RepeatedUniverse(number));
        }
        let slots = u32::try_from(size)
            .ok()
            .filter(|size| *size > 0)
            .ok_or(ControllerSkip::EmptyOutput)?;
        if slots > 512 {
            return Err(ControllerSkip::LargeOutput(slots));
        }
        universes.push((number as u16, Universe { start, slots }));
    }
    for (index, (_, a)) in universes.iter().enumerate() {
        for (_, b) in &universes[index + 1..] {
            if a.start < b.start + b.slots && b.start < a.start + a.slots {
                return Err(ControllerSkip::OverlappingUniverses);
            }
        }
    }
    Ok((
        ControllerProtocol::E131(E131Config {
            sender_name: "Donder".to_string(),
            bind_address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            priority,
            mode,
        }),
        universes,
    ))
}

pub(crate) fn read(
    controllers: &[ConfigController],
    modules: &HashMap<String, Element>,
    disabled: &HashSet<String>,
) -> Controllers {
    let mut result = Controllers {
        controllers: Vec::new(),
        slots: HashMap::new(),
    };
    for (index, controller) in controllers.iter().enumerate() {
        let read = protocol(
            controller,
            modules.get(&controller.hardware_instance_id),
            disabled.contains(&controller.id),
        );
        let (protocol, outputs) = match read {
            Ok((protocol, universes)) => {
                for (universe, (_, entry)) in universes.iter().enumerate() {
                    // Outputs past the controller's last are sent as zero.
                    let end = (entry.start + entry.slots).min(controller.outputs.len() as u32);
                    for output in entry.start..end {
                        result.slots.insert(
                            controller.outputs[output as usize].clone(),
                            Slot {
                                controller: index,
                                output: universe,
                                offset: output - entry.start,
                            },
                        );
                    }
                }
                let outputs = universes
                    .iter()
                    .map(|(number, entry)| ProfileOutput {
                        universe: *number,
                        channels: entry.slots as u16,
                    })
                    .collect();
                (Ok(protocol), outputs)
            }
            Err(reason) => {
                // Its outputs still say whose they are, so a pixel on them
                // is reported as on a controller that is not imported.
                for (output, id) in controller.outputs.iter().enumerate() {
                    result.slots.insert(
                        id.clone(),
                        Slot {
                            controller: index,
                            output: 0,
                            offset: output as u32,
                        },
                    );
                }
                (Err(reason), Vec::new())
            }
        };
        result.controllers.push(ProfileController {
            name: controller.name.clone(),
            description: None,
            protocol,
            outputs,
        });
    }
    result
}

/// A setting read from the controller's module data.
fn field<T>(result: Result<T, contract::FieldError>) -> Result<T, ControllerSkip> {
    result.map_err(|error| ControllerSkip::Setting(error.field, error.value))
}
