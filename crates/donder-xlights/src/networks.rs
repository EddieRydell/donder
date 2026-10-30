//! `xlights_networks.xml`: the controllers and the outputs each sends.
//!
//! xLights numbers every channel of the show from 1: controllers in file
//! order, each controller's outputs in order, each output taking its
//! `MaxChannels`. Model start channels resolve to these absolute numbers.

use crate::xml::Element;

/// The most universes one output sends: every E1.31 universe.
const MOST_UNIVERSES: u32 = 63_999;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Protocol {
    E131,
    ArtNet,
    /// A protocol Donder does not send, by its xLights name.
    Other(String),
}

pub(crate) struct NetworkController {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) id: Option<i64>,
    pub(crate) ip: String,
    pub(crate) protocol: Protocol,
    pub(crate) priority: Option<u32>,
    pub(crate) outputs: Vec<Output>,
}

pub(crate) struct Output {
    /// The universe for E1.31 and Art-Net.
    pub(crate) universe: u32,
    pub(crate) channels: u32,
    /// The output's first absolute channel, from 1.
    pub(crate) start: u32,
}

#[derive(Default)]
pub(crate) struct Networks {
    pub(crate) controllers: Vec<NetworkController>,
}

/// An absolute channel's place on a controller output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Slot {
    pub(crate) controller: usize,
    pub(crate) output: usize,
    /// From 0 within the output.
    pub(crate) offset: u32,
}

fn protocol(name: &str) -> Protocol {
    match name {
        "E131" => Protocol::E131,
        "ArtNet" => Protocol::ArtNet,
        other => Protocol::Other(other.to_string()),
    }
}

fn number<T: std::str::FromStr>(element: &Element, name: &str) -> Result<Option<T>, String> {
    element
        .attribute(name)
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value
                .trim()
                .parse()
                .map_err(|_| format!("`{name}` is `{value}`, not a whole number"))
        })
        .transpose()
}

/// The outputs one `<network>` element describes: an E1.31 output may carry
/// several consecutive universes.
fn outputs(network: &Element, next: &mut u32) -> Result<Vec<Output>, String> {
    let channels = number(network, "MaxChannels")?.unwrap_or(512);
    let universe: u32 = number(network, "BaudRate")?.unwrap_or(1);
    let count: u32 = number(network, "NumUniverses")?.unwrap_or(1).max(1);
    if count > MOST_UNIVERSES {
        return Err(format!(
            "`NumUniverses` is {count}; no output sends more than {MOST_UNIVERSES}"
        ));
    }
    (0..count)
        .map(|index| {
            let output = Output {
                universe: universe
                    .checked_add(index)
                    .ok_or_else(|| format!("universe {universe} and on is past the last"))?,
                channels,
                start: *next,
            };
            *next = next
                .checked_add(channels)
                .ok_or("the outputs number more channels than xLights can")?;
            Ok(output)
        })
        .collect()
}

impl Networks {
    pub(crate) fn read(root: &Element) -> Result<Self, String> {
        let mut next = 1;
        let mut controllers = Vec::new();
        for controller in root.children("Controller") {
            let name = controller.attribute("Name").unwrap_or_default().to_string();
            let mut outputs_of = Vec::new();
            for network in controller.children("network") {
                outputs_of.extend(outputs(network, &mut next)?);
            }
            let first_type = controller
                .child("network")
                .and_then(|network| network.attribute("NetworkType"));
            controllers.push(NetworkController {
                description: controller
                    .attribute("Description")
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_string),
                id: number(controller, "Id")?,
                ip: controller
                    .attribute("IP")
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                protocol: protocol(
                    controller
                        .attribute("Protocol")
                        .or(first_type)
                        .unwrap_or_default(),
                ),
                priority: number(controller, "Priority")?,
                outputs: outputs_of,
                name,
            });
        }
        // Files from before xLights had controllers list their outputs alone;
        // consecutive outputs of one type to one address are one controller.
        for network in root.children("network") {
            let ip = network
                .attribute("ComPort")
                .unwrap_or_default()
                .trim()
                .to_string();
            let network_protocol = protocol(network.attribute("NetworkType").unwrap_or_default());
            let outputs_of = outputs(network, &mut next)?;
            match controllers.last_mut() {
                Some(last) if last.ip == ip && last.protocol == network_protocol => {
                    last.outputs.extend(outputs_of)
                }
                _ => controllers.push(NetworkController {
                    name: ip.clone(),
                    description: None,
                    id: None,
                    ip,
                    protocol: network_protocol,
                    priority: None,
                    outputs: outputs_of,
                }),
            }
        }
        Ok(Self { controllers })
    }

    pub(crate) fn slot(&self, channel: u32) -> Option<Slot> {
        self.controllers
            .iter()
            .enumerate()
            .flat_map(|(controller, value)| {
                value
                    .outputs
                    .iter()
                    .enumerate()
                    .map(move |(output, value)| (controller, output, value))
            })
            .find(|(_, _, output)| {
                channel >= output.start && channel < output.start + output.channels
            })
            .map(|(controller, output, value)| Slot {
                controller,
                output,
                offset: channel - value.start,
            })
    }

    /// The first absolute channel of `universe`, on the controller at `ip`
    /// when one is named. Outputs that have no universe go by their
    /// controller's id instead.
    pub(crate) fn universe_start(&self, ip: Option<&str>, universe: u32) -> Option<u32> {
        self.controllers
            .iter()
            .filter(|controller| ip.is_none_or(|ip| controller.ip == ip))
            .find_map(|controller| match controller.protocol {
                Protocol::E131 | Protocol::ArtNet => controller
                    .outputs
                    .iter()
                    .find(|output| output.universe == universe)
                    .map(|output| output.start),
                Protocol::Other(_) => (controller.id == Some(i64::from(universe)))
                    .then(|| controller.outputs.first().map(|output| output.start))
                    .flatten(),
            })
    }

    pub(crate) fn controller_start(&self, name: &str) -> Option<u32> {
        self.controllers
            .iter()
            .find(|controller| controller.name == name)
            .and_then(|controller| controller.outputs.first())
            .map(|output| output.start)
    }
}
