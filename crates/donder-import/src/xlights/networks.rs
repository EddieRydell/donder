//! `xlights_networks.xml`: the controllers and the outputs each sends, as
//! xLights' `OutputManager::Load` reads them.
//!
//! xLights numbers every channel of the show from 1: controllers in file
//! order, each controller's outputs in order, each output taking its
//! channels, whether or not the controller is active. Model start channels
//! resolve to these absolute numbers.

pub(crate) use crate::setup::Slot;
use crate::xlights::read::{Reader, strtol};
use crate::xml::Element;

/// The output types xLights loads. xLights: `Output::Create`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputType {
    E131,
    ArtNet,
    Kinet,
    Zcpp,
    Ddp,
    Null,
    Dmx,
    XxxSerial,
    Pixelnet,
    OpenPixelnet,
    Lor,
    LorOptimised,
    DLight,
    Renard,
    OpenDmx,
    GenericSerial,
    XxxEthernet,
    Opc,
    Twinkly,
}

impl OutputType {
    /// xLights: `Output::Create`, which takes any `Sy… Ethernet` as xxx
    /// Ethernet.
    fn from_name(name: &str) -> Option<Self> {
        if name.ends_with(" Ethernet") && name.starts_with("Sy") {
            return Some(Self::XxxEthernet);
        }
        Some(match name {
            "E131" => Self::E131,
            "ZCPP" => Self::Zcpp,
            "NULL" => Self::Null,
            "ArtNet" => Self::ArtNet,
            "KINET" => Self::Kinet,
            "DDP" => Self::Ddp,
            "DMX" => Self::Dmx,
            "xxx Serial" => Self::XxxSerial,
            "OPC" => Self::Opc,
            "Pixelnet" => Self::Pixelnet,
            "LOR" => Self::Lor,
            "LOR Optimised" => Self::LorOptimised,
            "D-Light" => Self::DLight,
            "Renard" => Self::Renard,
            "OpenDMX" => Self::OpenDmx,
            "Pixelnet-Open" => Self::OpenPixelnet,
            "Generic Serial" => Self::GenericSerial,
            "xxx Ethernet" => Self::XxxEthernet,
            "Twinkly" => Self::Twinkly,
            _ => return None,
        })
    }

    /// xLights: `Output::IsIpOutput`, the outputs addressed by IP and
    /// universe (`IPOutput`).
    fn is_ip(self) -> bool {
        matches!(
            self,
            Self::E131
                | Self::ArtNet
                | Self::Kinet
                | Self::Zcpp
                | Self::Ddp
                | Self::XxxEthernet
                | Self::Opc
                | Self::Twinkly
        )
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::E131 => "E1.31",
            Self::ArtNet => "Art-Net",
            Self::Kinet => "KiNET",
            Self::Zcpp => "ZCPP",
            Self::Ddp => "DDP",
            Self::Null => "NULL",
            Self::Dmx => "DMX",
            Self::XxxSerial => "xxx Serial",
            Self::Pixelnet => "Pixelnet",
            Self::OpenPixelnet => "Pixelnet-Open",
            Self::Lor => "LOR",
            Self::LorOptimised => "LOR Optimised",
            Self::DLight => "D-Light",
            Self::Renard => "Renard",
            Self::OpenDmx => "OpenDMX",
            Self::GenericSerial => "Generic Serial",
            Self::XxxEthernet => "xxx Ethernet",
            Self::Opc => "OPC",
            Self::Twinkly => "Twinkly",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Output {
    pub(crate) kind: OutputType,
    /// The universe of an IP output, or the id of another. xLights:
    /// `Output::_universe`.
    pub(crate) universe: i32,
    pub(crate) ip: String,
    pub(crate) channels: u32,
    /// The output's first absolute channel, from 1.
    pub(crate) start: u32,
}

/// The controller types xLights loads. xLights: `Controller::Create`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControllerType {
    Ethernet,
    Serial,
    Null,
}

pub(crate) struct NetworkController {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) kind: ControllerType,
    /// xLights: `ControllerEthernet::_type`, the `Protocol` attribute: what
    /// the controller sends, as an output type.
    pub(crate) protocol: Option<OutputType>,
    /// The `Protocol` as written.
    pub(crate) protocol_name: String,
    /// The `Protocol` is `Player Only`.
    pub(crate) player_only: bool,
    pub(crate) ip: String,
    pub(crate) priority: i32,
    /// xLights: `Controller::IsActive`.
    pub(crate) active: bool,
    pub(crate) outputs: Vec<Output>,
}

/// A `<network>` of the older networks file, kept to convert start
/// channels written against it. xLights: `OutputManager::_conversionOutputs`.
struct Converted {
    kind: OutputType,
    ip: String,
    universe: i32,
    /// xLights: `Output::_description_CONVERT`.
    description: String,
    controller: usize,
    /// Its first output among the controller's outputs.
    output: usize,
}

#[derive(Default)]
pub(crate) struct Networks {
    pub(crate) controllers: Vec<NetworkController>,
    converted: Vec<Converted>,
}

/// One `<network>` element as an output, or none for a type xLights does
/// not load. xLights: `Output::Output(node)`, `IPOutput::IPOutput(node)`
/// and the E1.31, Art-Net, KiNET and xxx Ethernet constructors.
fn output(network: &Element) -> Option<Output> {
    let reader = Reader::new(network);
    let kind = OutputType::from_name(reader.as_string("NetworkType", ""))?;
    let mut channels = reader.as_int("MaxChannels", 0);
    if matches!(
        kind,
        OutputType::E131 | OutputType::ArtNet | OutputType::Kinet
    ) && channels > 512
    {
        channels = 512;
    }
    // xLights: IPOutput reads BaudRate; xxxEthernetOutput, NullOutput and
    // SerialOutput set it from their Id (SetId).
    let universe = match kind {
        OutputType::XxxEthernet if reader.as_int("Port", -1) != -1 => reader.as_int("Port", 0),
        OutputType::XxxEthernet => reader.as_int("Id", 0),
        kind if kind.is_ip() => reader.as_int("BaudRate", 1),
        OutputType::Null => reader.as_int("Id", 64001),
        _ => reader.as_int("Id", 0),
    };
    Some(Output {
        kind,
        universe,
        ip: if kind.is_ip() {
            reader.as_string("ComPort", "").to_string()
        } else {
            String::new()
        },
        channels: channels.max(0) as u32,
        start: 0,
    })
}

/// xLights: `Controller::EncodeActiveState` and `IsActive`; "xLights Only"
/// is active in xLights itself.
fn active(reader: &Reader) -> bool {
    match reader.as_string("ActiveState", "") {
        "" => reader.as_string("Active", "1") != "0",
        "Inactive" => false,
        _ => true,
    }
}

/// xLights: `UnXmlSafe`, which the older networks file's descriptions
/// pass through.
fn unxml_safe(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut result = text.to_string();
    for code in 0..32u8 {
        result = result.replace(&format!("&#{code}"), &char::from(code).to_string());
    }
    for (from, to) in [
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&apos;", "'"),
        ("&quot;", "\""),
        ("&amp;", "&"),
    ] {
        result = result.replace(from, to);
    }
    result
}

/// xLights: `XmlSafe`, whose signed `char` drops every byte of a
/// character outside ASCII along with control characters.
fn xml_safe(text: &str) -> String {
    let mut result = String::new();
    for &byte in text.as_bytes() {
        match byte {
            9 | 10 | 13 => result.push_str(&format!("&#{byte};")),
            0..32 | 128.. => {}
            b'&' => result.push_str("&amp;"),
            b'<' => result.push_str("&lt;"),
            b'>' => result.push_str("&gt;"),
            b'\'' => result.push_str("&apos;"),
            b'"' => result.push_str("&quot;"),
            _ => result.push(char::from(byte)),
        }
    }
    result
}

/// A unique name for a controller converted from the older networks file.
/// xLights: `OutputManager::UniqueName`.
fn unique_name(prefix: &str, controllers: &[NetworkController]) -> String {
    let taken = |name: &str| controllers.iter().any(|controller| controller.name == name);
    if !taken(prefix) {
        return prefix.to_string();
    }
    (1..)
        .map(|index| format!("{prefix}_{index}"))
        .find(|name| !taken(name))
        .unwrap_or_default()
}

impl Networks {
    /// xLights: `OutputManager::Load`.
    pub(crate) fn read(root: &Element) -> Self {
        let mut controllers: Vec<NetworkController> = Vec::new();
        let mut converted = Vec::new();
        // The older form, top-level `<network>` elements, is grouped into
        // controllers as xLights converts them.
        let mut converting: Option<usize> = None;
        let (mut last_type, mut last_port, mut last_universe) = (String::new(), String::new(), -1);
        for element in &root.children {
            match element.name.as_str() {
                "network" => {
                    let reader = Reader::new(element);
                    let Some(output) = output(element) else {
                        converting = None;
                        continue;
                    };
                    let mut kind = reader.as_string("NetworkType", "").to_string();
                    if kind.ends_with(" Ethernet") && kind.starts_with("Sy") {
                        kind = "xxx Ethernet".to_string();
                    }
                    let port = reader.as_string("ComPort", "").to_string();
                    let mut universe = reader.as_int("BaudRate", -1);
                    let count = reader.as_int("NumUniverses", 1);
                    if kind == "xxx Ethernet" {
                        universe = reader.as_int("Port", 1);
                    }
                    if kind != last_type
                        || last_port != port
                        || (last_universe != -1 && universe != -1 && universe != last_universe + 1)
                    {
                        converting = None;
                    }
                    if converting.is_none() {
                        if matches!(output.kind, OutputType::Opc | OutputType::Twinkly) {
                            continue;
                        }
                        let controller_type = match output.kind {
                            OutputType::Null => ControllerType::Null,
                            kind if kind.is_ip() => ControllerType::Ethernet,
                            _ => ControllerType::Serial,
                        };
                        controllers.push(NetworkController {
                            name: String::new(),
                            description: None,
                            kind: controller_type,
                            protocol: Some(output.kind),
                            protocol_name: output.kind.name().to_string(),
                            player_only: false,
                            ip: output.ip.clone(),
                            priority: reader.as_int("Priority", 100),
                            active: reader.as_string("Enabled", "Yes") == "Yes",
                            outputs: Vec::new(),
                        });
                        converting = Some(controllers.len() - 1);
                    }
                    let index = converting.unwrap_or_default();
                    let first = controllers[index].outputs.is_empty();
                    let description = unxml_safe(reader.as_string("Description", ""));
                    converted.push(Converted {
                        kind: output.kind,
                        ip: output.ip.clone(),
                        universe: output.universe,
                        description: description.clone(),
                        controller: index,
                        output: controllers[index].outputs.len(),
                    });
                    // xLights: E131Output::CreateMultiUniverses_CONVERT and
                    // ControllerEthernet::Convert.
                    if output.kind == OutputType::E131 && count > 1 {
                        for offset in 0..count {
                            controllers[index].outputs.push(Output {
                                universe: output.universe + offset,
                                ..output.clone()
                            });
                        }
                    } else {
                        controllers[index].outputs.push(output);
                    }
                    if first {
                        let description = description.as_str();
                        let prefix = if description.is_empty() {
                            "Unnamed"
                        } else {
                            description
                        };
                        controllers[index].name = unique_name(prefix, &controllers);
                    }
                    last_type = kind;
                    last_port = port;
                    last_universe = universe + count - 1;
                }
                "Controller" => {
                    let reader = Reader::new(element);
                    let kind = match reader.as_string("Type", "") {
                        "Null" => ControllerType::Null,
                        "Ethernet" => ControllerType::Ethernet,
                        "Serial" => ControllerType::Serial,
                        _ => continue,
                    };
                    let protocol_text = reader.as_string("Protocol", "");
                    let mut outputs: Vec<Output> = element
                        .children
                        .iter()
                        .filter(|child| child.name == "network")
                        .filter_map(output)
                        .collect();
                    // xLights: ControllerSerial gives its output its own Id.
                    if let (ControllerType::Serial, Some(first)) = (kind, outputs.first_mut()) {
                        first.universe = reader.as_int("Id", 64001);
                    }
                    controllers.push(NetworkController {
                        name: crate::xlights::read::trim(reader.as_string("Name", "Controller_"))
                            .to_string(),
                        description: Some(crate::xlights::read::trim(
                            reader.as_string("Description", ""),
                        ))
                        .filter(|text| !text.is_empty())
                        .map(str::to_string),
                        kind,
                        protocol: OutputType::from_name(protocol_text),
                        protocol_name: protocol_text.to_string(),
                        player_only: protocol_text == "Player Only",
                        ip: reader.as_string("IP", "").to_string(),
                        priority: reader.as_int("Priority", 100),
                        active: active(&reader),
                        outputs,
                    });
                }
                _ => {}
            }
        }
        // xLights: OutputManager::SomethingChanged.
        let mut next: u64 = 1;
        for controller in &mut controllers {
            for output in &mut controller.outputs {
                output.start = next.min(u64::from(u32::MAX)) as u32;
                next += u64::from(output.channels);
            }
        }
        Self {
            controllers,
            converted,
        }
    }

    /// A model's start channel as xLights converts it when the networks
    /// file had the older form: `N:k`, channel `k` of the Nth `<network>`,
    /// becomes `#ip:universe:k` (`#universe:k` when multicast) for E1.31 and
    /// Art-Net and `!controller:k` for the rest, and `!description:k` names
    /// the controller it became. xLights: `OutputManager::ConvertStartChannel`
    /// and `ConvertModelStartChannels`, which also convert the `String`
    /// start channels of an `Advanced` model, refused here.
    pub(crate) fn start_channel(&self, start: &str) -> String {
        if self.converted.is_empty() {
            return start.to_string();
        }
        let text = unxml_safe(start);
        let parts: Vec<&str> = text.split(':').collect();
        let [number, channel] = parts[..] else {
            return start.to_string();
        };
        let channel = strtol(channel) as i32;
        let converted = match number.as_bytes().first() {
            Some(first) if first.is_ascii_digit() => {
                let number = strtol(number) as i32;
                usize::try_from(number)
                    .ok()
                    .filter(|number| *number > 0)
                    .and_then(|number| self.converted.get(number - 1))
                    .map(|output| match output.kind {
                        OutputType::E131 | OutputType::ArtNet if output.ip == "MULTICAST" => {
                            format!("#{}:{channel}", output.universe)
                        }
                        OutputType::E131 | OutputType::ArtNet => {
                            format!("#{}:{}:{channel}", output.ip, output.universe)
                        }
                        _ => format!("!{}:{channel}", self.controllers[output.controller].name),
                    })
            }
            Some(b'!') => self
                .converted
                .iter()
                .find(|output| output.description == number[1..])
                .map(|output| {
                    let controller = &self.controllers[output.controller];
                    let before: u64 = controller.outputs[..output.output]
                        .iter()
                        .map(|output| u64::from(output.channels))
                        .sum();
                    format!(
                        "!{}:{}",
                        controller.name,
                        (before as i32).wrapping_add(channel)
                    )
                }),
            _ => None,
        };
        converted.map_or_else(|| start.to_string(), |text| xml_safe(&text))
    }

    /// The output holding absolute channel `channel`, and where in it.
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
                channel >= output.start
                    && u64::from(channel) < u64::from(output.start) + u64::from(output.channels)
            })
            .map(|(controller, output, value)| Slot {
                controller,
                output,
                offset: channel - value.start,
            })
    }

    /// The first channel of the IP output sending `universe`, at `ip` when
    /// one is named. xLights: `OutputManager::GetAbsoluteChannel(ip,
    /// universe, 0)`.
    pub(crate) fn universe_start(&self, ip: &str, universe: i32) -> Option<u32> {
        self.controllers
            .iter()
            .flat_map(|controller| &controller.outputs)
            .filter(|output| ip.is_empty() || (output.kind.is_ip() && output.ip == ip))
            .find(|output| output.universe == universe)
            .map(|output| output.start)
    }

    /// The first channel of the controller named `name`, unless it only
    /// plays. xLights: `Controller::GetStartChannel`, as
    /// `Model::GetNumberFromChannelString` uses it.
    pub(crate) fn controller_start(&self, name: &str) -> Option<u32> {
        self.controllers
            .iter()
            .find(|controller| controller.name == name)
            .filter(|controller| !controller.player_only)
            .and_then(|controller| controller.outputs.first())
            .map(|output| output.start)
    }
}
