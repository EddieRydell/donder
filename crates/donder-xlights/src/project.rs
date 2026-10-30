//! An xLights show as the layout, controllers and patch of a Donder project.

use crate::channels::{self, ModelChannels};
use crate::fit::{self, Pixel};
use crate::models::{self, Local, Spec};
use crate::networks::{Networks, Protocol, Slot};
use crate::report::{
    DropReason, DroppedMember, ImportOptions, ImportReport, Scale, SkipReason, Skipped,
    TYPICAL_PIXEL_SPACING, Unrouted, UnroutedReason,
};
use crate::xml::{self, Element};
use camino::{Utf8Path, Utf8PathBuf};
use donder_model::{
    ArtNetConfig, ArtNetMode, Controller, ControllerId, ControllerPort, ControllerPortAddress,
    ControllerPortId, ControllerProtocol, E131Config, E131Mode, FixtureDefinition,
    FixtureInstanceId, FixtureTarget, FixtureTransform, Layout, LayoutFixture, LayoutFixtureKind,
    LayoutId, ObjectId, Owned, OwnedMember, OwnershipSite, Patch, PatchId, PixelRoute,
    PixelRouteId, PixelSpan, ProjectEdit, Setup,
};
use donder_project_io::{DocumentId, ProjectSession};
use donder_runtime_types::{Identifier, PixelEncoding};
use glam::DVec3;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub const LAYOUT_FILE: &str = "xlights_rgbeffects.xml";
pub const NETWORKS_FILE: &str = "xlights_networks.xml";

#[derive(Debug)]
pub enum ImportError {
    Read {
        path: Utf8PathBuf,
        message: String,
    },
    Xml {
        path: Utf8PathBuf,
        message: String,
    },
    /// The project's setup already has fixtures, routes or controllers.
    NotEmpty,
    /// A controller setting Donder cannot send.
    Controller {
        name: String,
        message: String,
    },
    /// The imported objects do not make a valid project.
    Project(String),
    /// Pixels must be some positive distance apart.
    PixelSpacing(f64),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, message } => write!(f, "Could not read {path}: {message}"),
            Self::Xml { path, message } => write!(f, "{path} is not readable XML: {message}"),
            Self::NotEmpty => write!(
                f,
                "This project's setup already has fixtures or controllers; import an xLights show into a new project."
            ),
            Self::Controller { name, message } => {
                write!(f, "Controller {name} cannot be imported: {message}")
            }
            Self::PixelSpacing(spacing) => write!(
                f,
                "Pixels cannot be {spacing} m apart; give a distance above zero."
            ),
            Self::Project(message) => {
                write!(f, "The imported show is not a valid project: {message}")
            }
        }
    }
}

fn read(path: &Utf8Path) -> Result<Element, ImportError> {
    let bytes = std::fs::read(path).map_err(|error| ImportError::Read {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    let text = String::from_utf8(bytes).map_err(|error| ImportError::Xml {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    xml::parse(&text).map_err(|message| ImportError::Xml {
        path: path.to_owned(),
        message,
    })
}

/// A model as far as it is imported: its pixels in channel order.
struct Imported {
    name: String,
    description: Option<String>,
    pixels: Vec<Pixel>,
    ellipses: Vec<models::Ellipse>,
    /// Each pixel's first absolute channel, when the model is patched.
    channels: Option<Vec<u32>>,
    encoding: PixelEncoding,
    gamma: f32,
    brightness: f32,
}

/// Slots from `0` up to `1` of an output, patched to the model named `2`.
type Claim = (u32, u32, String);

/// Channels from a model's first to its last.
fn span(spec: &Spec, local: Option<&Local>) -> Option<u32> {
    if let Some(count) = models::channel_count(spec) {
        return Some(count);
    }
    let local = local?;
    let per_node = match models::encoding(spec) {
        Ok(encoding) => encoding.channel_order().len() as u32,
        // Lights that are not pixels still take their channels.
        Err(_) => match spec.element.attribute("StringType").unwrap_or_default() {
            "3 Channel RGB" => return Some(3 * local.strings),
            "4 Channel RGBW" | "4 Channel WRGB" => return Some(4 * local.strings),
            "Node Single Color" => 1,
            kind if kind.starts_with("Single Color") || kind == "Strobes" => {
                return Some(local.strings);
            }
            _ => return None,
        },
    };
    local
        .nodes
        .iter()
        .map(|node| (node.string * local.string_nodes + node.offset + 1) * per_node)
        .max()
}

/// Meters per layout unit: the ruler's, or pixels the typical distance
/// apart.
fn scale(root: &Element, locals: &[Option<Local>], options: &ImportOptions) -> (f64, Scale) {
    if options.pixel_spacing.is_none()
        && let Some(meters) = ruler(root)
    {
        return (meters, Scale::Ruler);
    }
    let spacing = options.pixel_spacing.unwrap_or(TYPICAL_PIXEL_SPACING);
    let mut gaps: Vec<f64> = locals
        .iter()
        .flatten()
        .flat_map(|local| {
            let transform = local.placement.transform();
            let mut nodes: Vec<_> = local.nodes.iter().collect();
            nodes.sort_by_key(|node| (node.string, node.offset));
            nodes
                .windows(2)
                .filter(|pair| pair[0].string == pair[1].string)
                .map(|pair| {
                    transform
                        .transform_point3(pair[0].position)
                        .distance(transform.transform_point3(pair[1].position))
                })
                .filter(|gap| *gap > 0.0)
                .collect::<Vec<_>>()
        })
        .collect();
    gaps.sort_by(f64::total_cmp);
    let typical = gaps.get(gaps.len() / 2).copied().unwrap_or(1.0);
    (spacing / typical, Scale::PixelSpacing(spacing))
}

fn ruler(root: &Element) -> Option<f64> {
    let ruler = root
        .child("view_objects")?
        .children("view_object")
        .find(|object| object.attribute("DisplayAs") == Some("Ruler"))?;
    let number = |name: &str| ruler.attribute(name)?.trim().parse::<f64>().ok();
    let units = match number("Units").unwrap_or(0.0) as u32 {
        0 => 1.0,
        1 => 0.01,
        2 => 0.001,
        3 => 0.9144,
        4 => 0.3048,
        _ => 0.0254,
    };
    let length = DVec3::new(number("X2")?, number("Y2")?, number("Z2").unwrap_or(0.0)).length();
    (length > 0.0).then(|| number("Length").unwrap_or(0.0) * units / length)
}

fn connection(spec: &Spec, name: &str, default: f32) -> f32 {
    spec.element
        .child("ControllerConnection")
        .and_then(|connection| {
            connection
                .attribute(name)
                .or_else(|| connection.attribute(&name.to_ascii_lowercase()))
        })
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default)
}

/// The Donder controllers, with each xLights output's port.
struct Controllers {
    controllers: Vec<(Identifier, Controller)>,
    /// By xLights controller and output.
    ports: HashMap<(usize, usize), (ControllerId, ControllerPortId, u32)>,
    skipped: Vec<(String, String)>,
}

fn controllers(
    networks: &Networks,
    mut new_id: impl FnMut() -> u32,
) -> Result<Controllers, ImportError> {
    let mut result = Controllers {
        controllers: Vec::new(),
        ports: HashMap::new(),
        skipped: Vec::new(),
    };
    let mut taken = HashSet::new();
    for (index, controller) in networks.controllers.iter().enumerate() {
        let invalid = |message: String| ImportError::Controller {
            name: controller.name.clone(),
            message,
        };
        let protocol = match &controller.protocol {
            Protocol::Other(protocol) => {
                result
                    .skipped
                    .push((controller.name.clone(), protocol.clone()));
                continue;
            }
            Protocol::E131 => {
                let mode = if controller.ip.eq_ignore_ascii_case("MULTICAST") {
                    E131Mode::Multicast
                } else {
                    E131Mode::Unicast {
                        destination: controller
                            .ip
                            .parse()
                            .map_err(|_| invalid(format!("its address is `{}`", controller.ip)))?,
                    }
                };
                let priority = controller.priority.unwrap_or(100);
                ControllerProtocol::E131(E131Config {
                    sender_name: "Donder".to_string(),
                    bind_address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    priority: u8::try_from(priority)
                        .ok()
                        .filter(|priority| (1..=200).contains(priority))
                        .ok_or_else(|| invalid(format!("its priority is {priority}")))?,
                    mode,
                })
            }
            Protocol::ArtNet => {
                let destination: IpAddr = controller
                    .ip
                    .parse()
                    .map_err(|_| invalid(format!("its address is `{}`", controller.ip)))?;
                ControllerProtocol::ArtNet(ArtNetConfig {
                    bind_address: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 6454),
                    destination: SocketAddr::new(destination, 6454),
                    mode: ArtNetMode::Unicast,
                })
            }
        };
        let id = ControllerId(new_id());
        let mut ports = Vec::new();
        for (output_index, output) in controller.outputs.iter().enumerate() {
            let universe = u16::try_from(output.universe)
                .map_err(|_| invalid(format!("universe {} is out of range", output.universe)))?;
            let address = match protocol {
                ControllerProtocol::ArtNet(_) => ControllerPortAddress::ArtNetPort(universe),
                _ => ControllerPortAddress::E131Universe(universe),
            };
            let slots = u16::try_from(output.channels)
                .ok()
                .filter(|slots| *slots <= 512)
                .ok_or_else(|| {
                    invalid(format!(
                        "universe {} has {} channels",
                        output.universe, output.channels
                    ))
                })?;
            let port = ControllerPortId(ports.len() as u32 + 1);
            ports.push(ControllerPort {
                id: port,
                name: donder_language::object_name(&format!("universe_{}", output.universe)),
                address,
                slot_count: slots,
            });
            result
                .ports
                .insert((index, output_index), (id, port, output.channels));
        }
        let name = donder_language::unique_name(
            donder_language::object_name(&controller.name).as_str(),
            |name| taken.contains(name),
        );
        taken.insert(name.as_str().to_string());
        result.controllers.push((
            name,
            Controller {
                id,
                description: controller.description.clone(),
                protocol,
                ports,
            },
        ));
    }
    Ok(result)
}

/// The routes of one model, or why some of its pixels have none.
fn routes(
    imported: &Imported,
    networks: &Networks,
    controllers: &Controllers,
) -> Result<Vec<(Slot, u32, u32)>, UnroutedReason> {
    let Some(channels) = &imported.channels else {
        return Ok(Vec::new());
    };
    let per_pixel = imported.encoding.channel_order().len() as u32;
    // Runs of pixels as the output slot of the first and how many.
    let mut runs: Vec<(Slot, u32, u32)> = Vec::new();
    for (pixel, &channel) in channels.iter().enumerate() {
        let slot = networks
            .slot(channel)
            .ok_or(UnroutedReason::OutsideOutputs)?;
        let Some(&(_, _, slots)) = controllers.ports.get(&(slot.controller, slot.output)) else {
            let protocol = match &networks.controllers[slot.controller].protocol {
                Protocol::Other(protocol) => protocol.clone(),
                _ => String::new(),
            };
            return Err(UnroutedReason::UnsupportedController(protocol));
        };
        if slot.offset + per_pixel > slots {
            return Err(UnroutedReason::SplitPixel);
        }
        match runs.last_mut() {
            Some((first, start, count))
                if first.controller == slot.controller
                    && first.output == slot.output
                    && first.offset + *count * per_pixel == slot.offset
                    && *start + *count == pixel as u32 =>
            {
                *count += 1
            }
            _ => runs.push((slot, pixel as u32, 1)),
        }
    }
    Ok(runs)
}

pub fn import_show(
    session: &mut ProjectSession,
    show: &Utf8Path,
    options: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    if let Some(spacing) = options.pixel_spacing
        && !(spacing.is_finite() && spacing > 0.0)
    {
        return Err(ImportError::PixelSpacing(spacing));
    }
    let root = read(&show.join(LAYOUT_FILE))?;
    let networks_path = show.join(NETWORKS_FILE);
    let networks = if networks_path.exists() {
        Networks::read(&read(&networks_path)?).map_err(|message| ImportError::Xml {
            path: networks_path,
            message,
        })?
    } else {
        Networks::default()
    };
    let mut report = ImportReport::default();

    let elements: Vec<&Element> = root
        .child("models")
        .map(|models| models.children("model").collect())
        .unwrap_or_default();
    let specs: Vec<Spec> = elements.iter().map(|element| Spec::new(element)).collect();
    let locals: Vec<Result<Local, SkipReason>> = specs.iter().map(models::layout).collect();
    let starts = channels::resolve(
        &specs
            .iter()
            .zip(&locals)
            .map(|(spec, local)| ModelChannels {
                name: spec.name.clone(),
                start: spec
                    .element
                    .attribute("StartChannel")
                    .unwrap_or("1")
                    .to_string(),
                span: span(spec, local.as_ref().ok()),
            })
            .collect::<Vec<_>>(),
        &networks,
    );
    let included: Vec<Option<Local>> = specs
        .iter()
        .zip(locals)
        .map(|(spec, local)| {
            let local = models::excluded(spec).and(local).and_then(|local| {
                models::encoding(spec)?;
                Ok(local)
            });
            match local {
                Ok(local) => Some(local),
                Err(reason) => {
                    report.skipped.push(Skipped {
                        model: spec.name.clone(),
                        reason,
                    });
                    None
                }
            }
        })
        .collect();
    let (meters, scale) = scale(&root, &included, options);
    report.meters_per_unit = meters;
    report.scale = Some(scale);

    let mut imported = Vec::new();
    for ((spec, local), start) in specs.iter().zip(included).zip(starts) {
        let Some(local) = local else { continue };
        let Ok(encoding) = models::encoding(spec) else {
            continue;
        };
        let per_pixel = encoding.channel_order().len() as u32;
        let transform = local.placement.transform();
        let mut nodes: Vec<_> = local.nodes.iter().collect();
        nodes.sort_by_key(|node| (node.string, node.offset));
        let channels = start.and_then(|start| {
            nodes
                .iter()
                .map(|node| {
                    node.string
                        .checked_mul(local.string_nodes)
                        .and_then(|before| before.checked_add(node.offset))
                        .and_then(|index| index.checked_mul(per_pixel))
                        .and_then(|offset| offset.checked_add(start))
                        .ok_or_else(|| {
                            UnroutedReason::OutOfRange(
                                spec.element
                                    .attribute("StartChannel")
                                    .unwrap_or("1")
                                    .to_string(),
                            )
                        })
                })
                .collect::<Result<Vec<u32>, _>>()
        });
        let channels = match channels {
            Ok(channels) => Some(channels),
            Err(reason) => {
                report.unrouted.push(Unrouted {
                    model: spec.name.clone(),
                    reason,
                });
                None
            }
        };
        let place = |position: DVec3| transform.transform_point3(position) * meters;
        imported.push(Imported {
            name: spec.name.clone(),
            description: spec
                .element
                .attribute("Description")
                .filter(|text| !text.trim().is_empty())
                .map(str::to_string),
            pixels: nodes
                .iter()
                .map(|node| Pixel {
                    position: place(node.position),
                    arc: node.arc.map(|arc| (arc.arc, arc.degrees)),
                })
                .collect(),
            ellipses: local
                .ellipses
                .iter()
                .map(|ellipse| models::Ellipse {
                    center: place(ellipse.center),
                    u: transform.transform_vector3(ellipse.u) * meters,
                    v: transform.transform_vector3(ellipse.v) * meters,
                })
                .collect(),
            channels,
            encoding,
            gamma: connection(spec, "gamma", 1.0),
            brightness: connection(spec, "Brightness", 100.0) / 100.0,
        });
    }

    let controllers = controllers(&networks, || session.project.new_id())?;
    report.skipped_controllers = controllers.skipped.clone();

    // Patch each model whose channels no earlier model has.
    let layout_id = LayoutId(session.project.new_id());
    let patch_id = PatchId(session.project.new_id());
    // By xLights controller and output, the slots each model has.
    let mut claimed: HashMap<(usize, usize), Vec<Claim>> = HashMap::new();
    let mut fixtures = Vec::new();
    let mut route_list = Vec::new();
    let mut by_name: HashMap<String, FixtureInstanceId> = HashMap::new();
    let mut taken: HashSet<String> = HashSet::new();
    for model in imported {
        let runs = match routes(&model, &networks, &controllers) {
            Ok(runs) => runs,
            Err(reason) => {
                report.unrouted.push(Unrouted {
                    model: model.name.clone(),
                    reason,
                });
                Vec::new()
            }
        };
        let per_pixel = model.encoding.channel_order().len() as u32;
        let overlap = runs.iter().find_map(|(slot, _, count)| {
            let (from, to) = (slot.offset, slot.offset + count * per_pixel);
            claimed
                .get(&(slot.controller, slot.output))?
                .iter()
                .find(|(start, end, _)| from < *end && *start < to)
                .map(|(_, _, owner)| owner.clone())
        });
        if let Some(owner) = overlap {
            report.skipped.push(Skipped {
                model: model.name,
                reason: SkipReason::SharesChannels(owner),
            });
            continue;
        }
        for (slot, _, count) in &runs {
            claimed
                .entry((slot.controller, slot.output))
                .or_default()
                .push((
                    slot.offset,
                    slot.offset + count * per_pixel,
                    model.name.clone(),
                ));
        }
        let id = FixtureInstanceId(fixtures.len() as u32 + 1);
        let origin = model.pixels[0].position;
        let elements = fit::shapes(&model.pixels, &model.ellipses, origin);
        let name = donder_language::unique_name(
            donder_language::object_name(&model.name).as_str(),
            |name| taken.contains(name),
        );
        taken.insert(name.as_str().to_string());
        by_name.insert(model.name.clone(), id);
        let pixel_count = model.pixels.len() as u32;
        for (slot, start, count) in runs {
            let (controller, port, _) = controllers.ports[&(slot.controller, slot.output)];
            route_list.push(PixelRoute {
                id: PixelRouteId(route_list.len() as u32 + 1),
                target: FixtureTarget {
                    layout: layout_id,
                    fixture: id,
                },
                pixels: (count != pixel_count).then_some(PixelSpan { start, count }),
                controller,
                port,
                start_slot: slot.offset as u16,
                encoding: model.encoding,
                gamma: model.gamma,
                brightness: model.brightness,
            });
        }
        report.pixels += model.pixels.len();
        fixtures.push(LayoutFixture {
            id,
            name,
            description: model.description,
            kind: LayoutFixtureKind::Fixture {
                definition: Owned::Inline(FixtureDefinition {
                    description: None,
                    elements,
                }),
                transform: FixtureTransform {
                    position: fit::fixture_position(origin),
                    ..FixtureTransform::default()
                },
            },
        });
    }
    report.fixtures = fixtures.len();
    report.routes = route_list.len();
    report.controllers = controllers.controllers.len();

    let (groups, root_members) = groups(&root, &by_name, fixtures.len(), &mut taken, &mut report);
    report.groups = groups.len();
    fixtures.extend(groups);
    let layout = Layout {
        id: layout_id,
        description: None,
        fixtures,
        root: root_members,
    };
    let patch = Patch {
        id: patch_id,
        description: None,
        routes: route_list,
    };
    insert(session, layout, patch, controllers.controllers)?;
    Ok(report)
}

/// The layout's groups, after its fixtures, and the layout's top level:
/// the groups no group contains, then the fixtures no group contains.
fn groups(
    root: &Element,
    fixtures: &HashMap<String, FixtureInstanceId>,
    fixture_count: usize,
    taken: &mut HashSet<String>,
    report: &mut ImportReport,
) -> (Vec<LayoutFixture>, Vec<FixtureInstanceId>) {
    let elements: Vec<&Element> = root
        .child("modelGroups")
        .map(|groups| groups.children("modelGroup").collect())
        .unwrap_or_default();
    let names: Vec<String> = elements
        .iter()
        .map(|group| {
            group
                .attribute("name")
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .collect();
    let group_index: HashMap<&str, usize> = names
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect();
    enum Member {
        Fixture(FixtureInstanceId),
        Group(usize),
    }
    let mut members: Vec<Vec<Member>> = Vec::new();
    for (group, element) in names.iter().zip(&elements) {
        let mut list = Vec::new();
        let mut seen = HashSet::new();
        for entry in element.attribute("models").unwrap_or_default().split(',') {
            let entry = entry.trim();
            if entry.is_empty() || !seen.insert(entry) {
                continue;
            }
            let member = if let Some(&fixture) = fixtures.get(entry) {
                Member::Fixture(fixture)
            } else if let Some(&index) = group_index.get(entry) {
                Member::Group(index)
            } else {
                report.dropped_members.push(DroppedMember {
                    group: group.clone(),
                    member: entry.to_string(),
                    reason: if entry.contains('/') {
                        DropReason::Submodel
                    } else {
                        DropReason::Missing
                    },
                });
                continue;
            };
            list.push(member);
        }
        members.push(list);
    }
    // Leave out members that would put a group inside itself.
    fn reaches(members: &[Vec<Member>], from: usize, to: usize, seen: &mut HashSet<usize>) -> bool {
        from == to
            || (seen.insert(from)
                && members[from].iter().any(|member| match member {
                    Member::Group(next) => reaches(members, *next, to, seen),
                    Member::Fixture(_) => false,
                }))
    }
    for group in 0..members.len() {
        let mut kept = Vec::new();
        for member in std::mem::take(&mut members[group]) {
            if let Member::Group(inner) = member
                && reaches(&members, inner, group, &mut HashSet::new())
            {
                report.dropped_members.push(DroppedMember {
                    group: names[group].clone(),
                    member: names[inner].clone(),
                    reason: DropReason::Loop,
                });
                continue;
            }
            kept.push(member);
        }
        members[group] = kept;
    }
    // Groups left empty are left out, which may empty the groups holding
    // them.
    let mut empty = vec![false; members.len()];
    loop {
        let mut changed = false;
        for group in 0..members.len() {
            if !empty[group]
                && members[group].iter().all(|member| match member {
                    Member::Group(inner) => empty[*inner],
                    Member::Fixture(_) => false,
                })
            {
                empty[group] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut ids = HashMap::new();
    for (group, is_empty) in empty.iter().enumerate() {
        if !is_empty {
            ids.insert(
                group,
                FixtureInstanceId((fixture_count + ids.len()) as u32 + 1),
            );
        }
    }
    let mut grouped = HashSet::new();
    let mut nested = HashSet::new();
    let mut items = Vec::new();
    for (group, list) in members.iter().enumerate() {
        let Some(&id) = ids.get(&group) else { continue };
        let list: Vec<FixtureInstanceId> = list
            .iter()
            .filter_map(|member| match member {
                Member::Fixture(fixture) => {
                    grouped.insert(*fixture);
                    Some(*fixture)
                }
                Member::Group(inner) => ids
                    .get(inner)
                    .inspect(|inner| {
                        nested.insert(**inner);
                    })
                    .copied(),
            })
            .collect();
        let name = donder_language::unique_name(
            donder_language::object_name(&names[group]).as_str(),
            |name| taken.contains(name),
        );
        taken.insert(name.as_str().to_string());
        items.push(LayoutFixture {
            id,
            name,
            description: None,
            kind: LayoutFixtureKind::Group { members: list },
        });
    }
    let mut root_members: Vec<FixtureInstanceId> = items
        .iter()
        .map(|item| item.id)
        .filter(|id| !nested.contains(id))
        .collect();
    let mut loose: Vec<FixtureInstanceId> = fixtures
        .values()
        .copied()
        .filter(|fixture| !grouped.contains(fixture))
        .collect();
    loose.sort();
    root_members.extend(loose);
    (items, root_members)
}

/// A new data document under `directory` named `stem`, or `stem_2` and so on
/// when that is taken.
fn new_document(
    session: &mut ProjectSession,
    directory: &str,
    objects: Vec<(ObjectId, Identifier)>,
) -> Result<DocumentId, ImportError> {
    for index in 1_u32.. {
        let stem = if index == 1 {
            "main".to_string()
        } else {
            format!("main_{index}")
        };
        let path = Utf8PathBuf::from(format!(
            "{directory}/{stem}{}",
            donder_language::data::DATA_DOCUMENT_SUFFIX
        ));
        if session
            .source
            .documents
            .contains_key(&session.source.project_document(path.clone()))
            || session.source.project_root().join(&path).exists()
        {
            continue;
        }
        return session
            .source
            .add_data_document(path, objects)
            .map_err(ImportError::Project);
    }
    Err(ImportError::Project("No document names remain.".into()))
}

/// Put the layout, patch and controllers in the project's setup, each in its
/// own document as Donder saves them.
fn insert(
    session: &mut ProjectSession,
    layout: Layout,
    patch: Patch,
    controllers: Vec<(Identifier, Controller)>,
) -> Result<(), ImportError> {
    let project = &session.project;
    let root = project.root().clone();
    let setup_id = match &root.setup {
        Owned::Inline(setup) => setup.id,
        Owned::Reference(id) => *id,
    };
    let setup = project
        .get::<Setup>(setup_id)
        .ok_or_else(|| ImportError::Project("The project has no setup.".into()))?
        .clone();
    let layout_id = match &setup.layout {
        Owned::Inline(layout) => layout.id,
        Owned::Reference(id) => *id,
    };
    let patch_id = match &setup.patch {
        Owned::Inline(patch) => patch.id,
        Owned::Reference(id) => *id,
    };
    let has_fixtures = project
        .get::<Layout>(layout_id)
        .is_some_and(|layout| !layout.fixtures.is_empty());
    let has_routes = project
        .get::<Patch>(patch_id)
        .is_some_and(|patch| !patch.routes.is_empty());
    if has_fixtures || has_routes || !setup.controllers.is_empty() {
        return Err(ImportError::NotEmpty);
    }
    let invalid = |error: donder_model::EditError| ImportError::Project(format!("{error:?}"));
    let names: Vec<(ObjectId, Identifier)> = controllers
        .iter()
        .map(|(name, controller)| (controller.id.into(), name.clone()))
        .collect();
    let mut edits: Vec<ProjectEdit> = vec![
        ProjectEdit::Put(layout.clone().into()),
        ProjectEdit::Put(patch.clone().into()),
    ];
    edits.extend(
        controllers
            .iter()
            .map(|(_, controller)| ProjectEdit::Put(controller.clone().into())),
    );
    edits.push(ProjectEdit::Put(
        Setup {
            layout: Owned::Reference(layout.id),
            patch: Owned::Reference(patch.id),
            controllers: controllers
                .iter()
                .map(|(_, controller)| OwnedMember::Reference(controller.id))
                .collect(),
            ..setup
        }
        .into(),
    ));
    session.project.apply_edits(edits).map_err(invalid)?;
    let setup_document = match &root.setup {
        Owned::Inline(_) => {
            let id = donder_model::make_reusable(
                &mut session.project,
                &OwnershipSite::Member {
                    owner: root.id.into(),
                    member: setup_id.into(),
                },
            )
            .map_err(invalid)?;
            new_document(
                session,
                "setups",
                vec![(id, donder_language::object_name("main"))],
            )?
        }
        Owned::Reference(id) => session
            .address_of((*id).into())
            .ok_or_else(|| ImportError::Project("The setup has no document.".into()))?
            .document_id()
            .clone(),
    };
    for (id, name) in names {
        session
            .source
            .add_object(&setup_document, id, name.as_str())
            .map_err(ImportError::Project)?;
    }
    new_document(
        session,
        "layouts",
        vec![(layout.id.into(), donder_language::object_name("main"))],
    )?;
    new_document(
        session,
        "patches",
        vec![(patch.id.into(), donder_language::object_name("main"))],
    )?;
    donder_project_io::maintain_ownership_sources(session)
        .map_err(|error| ImportError::Project(error.to_string()))?;
    Ok(())
}
