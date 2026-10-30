//! An xLights show as the layout, controllers and patch of a Donder project.

use crate::channels::{self, ModelChannels};
use crate::fit::{self, Pixel};
use crate::kind::ModelType;
use crate::models::{self, Dimming, Local};
use crate::networks::{ControllerType, Networks, OutputType, Slot};
use crate::read::Reader;
use crate::report::{
    ControllerSkip, DropReason, DroppedMember, ImportOptions, ImportReport, Scale, SkipReason,
    Skipped, SkippedController, TYPICAL_PIXEL_SPACING, Unrouted, UnroutedReason,
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
use glam::{DVec3, Vec3};
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
    /// The imported objects do not make a valid project.
    Project(String),
    /// Pixels must be some positive distance apart.
    PixelSpacing(f64),
    /// The layout's ruler measures in units xLights does not name.
    RulerUnits(i32),
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
            Self::PixelSpacing(spacing) => write!(
                f,
                "Pixels cannot be {spacing} m apart; give a distance above zero."
            ),
            Self::RulerUnits(units) => write!(
                f,
                "The layout's ruler measures in units {units}, which xLights does not name; set --pixel-spacing instead."
            ),
            Self::Project(message) => {
                write!(f, "The imported show is not a valid project: {message}")
            }
        }
    }
}

fn read_file(path: &Utf8Path) -> Result<Element, ImportError> {
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

/// The previews whose models the importer places: xLights' default
/// preview, and models shown in all of them. xLights: `LayoutGroup`.
const DEFAULT_PREVIEWS: [&str; 2] = ["Default", "All Previews"];

/// A model of the show as the importer reads it, before it becomes a
/// fixture.
pub(crate) struct ShowModel {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    /// Its nodes, or why it is not imported.
    pub(crate) local: Result<Local, SkipReason>,
    /// Its first absolute channel, from 1, or why it has none.
    pub(crate) start: Result<u32, UnroutedReason>,
    /// Its start channel as written.
    pub(crate) start_channel: String,
    pub(crate) encoding: Option<PixelEncoding>,
    pub(crate) dimming: Option<Dimming>,
}

impl ShowModel {
    /// Each node's position in xLights layout units, transformed in single
    /// precision as xLights transforms each light, then averaged.
    pub(crate) fn positions(&self) -> Vec<DVec3> {
        let Ok(local) = &self.local else {
            return Vec::new();
        };
        let transform = local.placement.transform(local.size);
        local
            .nodes
            .iter()
            .map(|node| {
                let sum: DVec3 = node
                    .lights
                    .iter()
                    .map(|light| {
                        let point = transform.transform_point3(Vec3::new(
                            light.x as f32,
                            light.y as f32,
                            light.z as f32,
                        ));
                        DVec3::new(f64::from(point.x), f64::from(point.y), f64::from(point.z))
                    })
                    .sum();
                sum / node.lights.len().max(1) as f64
            })
            .collect()
    }
}

/// The show's models as xLights loads them, each read and its start
/// channel resolved, and the models xLights does not load.
pub(crate) fn read_models(root: &Element, networks: &Networks) -> (Vec<ShowModel>, Vec<Skipped>) {
    let mut unloaded = Vec::new();
    let elements: Vec<&Element> = root
        .child("models")
        .map(|models| models.children("model").collect())
        .unwrap_or_default();
    let mut read = Vec::new();
    for element in elements {
        // xLights: ModelManager::LoadModels skips models with no name.
        let name = models::name(element);
        if name.is_empty() {
            continue;
        }
        let reader = Reader::new(element);
        let display = reader.as_string("DisplayAs", "DisplayAs Missing");
        let Some(kind) = ModelType::from_display(display, reader.as_string("DmxStyle", "")) else {
            unloaded.push(Skipped {
                model: name,
                reason: SkipReason::UnknownType(display.to_string()),
            });
            continue;
        };
        if kind == ModelType::ModelGroup {
            unloaded.push(Skipped {
                model: name,
                reason: SkipReason::Type("ModelGroup"),
            });
            continue;
        }
        read.push(models::read(element, kind));
    }
    let names: HashSet<&str> = read
        .iter()
        .map(|model| model.common.name.as_str())
        .collect();
    let groups: HashSet<String> = root
        .child("modelGroups")
        .into_iter()
        .flat_map(|groups| groups.children("modelGroup"))
        .map(models::name)
        .collect();
    let starts = channels::resolve(
        &read
            .iter()
            .map(|model| ModelChannels {
                name: model.common.name.clone(),
                start: model.common.start_channel.to_string(),
                controller: model.common.controller.clone(),
                span: model.span.as_ref().ok().copied(),
            })
            .collect::<Vec<_>>(),
        &groups,
        networks,
    );
    let models = read
        .iter()
        .zip(starts)
        .map(|(model, start)| {
            let common = &model.common;
            // xLights: ModelManager::LoadModels clears a ShadowModelFor
            // naming no model.
            let shadow = common
                .shadow_of
                .as_deref()
                .is_some_and(|other| names.contains(other));
            // xLights resolves a start channel that counts from the model
            // itself through its own half-set state; refuse it.
            let own = matches!(&start, Err(UnroutedReason::ChainLoop));
            let duplicate = matches!(&start, Err(UnroutedReason::Duplicate(_)));
            let local = if duplicate {
                Err(SkipReason::DuplicateName)
            } else if own {
                Err(SkipReason::Setting(
                    "start channel that counts from the model itself",
                ))
            } else if !DEFAULT_PREVIEWS.contains(&common.layout_group) {
                Err(SkipReason::OtherPreview(common.layout_group.to_string()))
            } else if shadow {
                Err(SkipReason::Shadow)
            } else if !common.active {
                Err(SkipReason::Inactive)
            } else {
                match &model.local {
                    Ok(_) => Ok(()),
                    Err(reason) => Err(reason.clone()),
                }
            };
            (local, start)
        })
        .collect::<Vec<_>>();
    let shows = read
        .into_iter()
        .zip(models)
        .map(|(model, (status, start))| ShowModel {
            name: model.common.name.clone(),
            description: model.common.description.clone(),
            local: status.and(model.local),
            start,
            start_channel: model.common.start_channel.to_string(),
            encoding: model.common.string_type.encoding,
            dimming: model.common.dimming.clone().ok().flatten(),
        })
        .collect();
    (shows, unloaded)
}

/// Layout units a meter, from the layout's last ruler. xLights:
/// `RulerObject::GetPerUnit`, whose static ruler is the last one loaded.
fn ruler(root: &Element) -> Result<Option<f64>, ImportError> {
    let Some(ruler) = root
        .child("view_objects")
        .into_iter()
        .flat_map(|objects| objects.children("view_object"))
        .filter(|object| object.attribute("DisplayAs") == Some("Ruler"))
        .last()
    else {
        return Ok(None);
    };
    let reader = Reader::new(ruler);
    let number = |name: &str, default: f32| reader.as_float(name, default).unwrap_or(default);
    let units = reader.as_int("Units", 0);
    let meters = match units {
        0 => 1.0,
        1 => 0.01,
        2 => 0.001,
        3 => 0.9144,
        4 => 0.3048,
        5 => 0.0254,
        other => return Err(ImportError::RulerUnits(other)),
    };
    let (dx, dy, dz) = (number("X2", 0.0), number("Y2", 0.0), number("Z2", 0.0));
    let length = number("Length", 1.0);
    let distance = (dx * dx + dy * dy + dz * dz).sqrt();
    let per_unit = if distance == 0.0 || length == 0.0 {
        1.0
    } else {
        length / distance
    };
    Ok(Some(f64::from(per_unit) * meters))
}

/// Meters per layout unit: from the pixel spacing when it is given, else
/// the ruler's, else pixels the typical distance apart.
fn scale(
    root: &Element,
    models: &[ShowModel],
    options: &ImportOptions,
) -> Result<(f64, Scale), ImportError> {
    if options.pixel_spacing.is_none()
        && let Some(meters) = ruler(root)?
    {
        return Ok((meters, Scale::Ruler));
    }
    let spacing = options.pixel_spacing.unwrap_or(TYPICAL_PIXEL_SPACING);
    let mut gaps: Vec<f64> = models
        .iter()
        .flat_map(|model| {
            model
                .positions()
                .windows(2)
                .map(|pair| pair[0].distance(pair[1]))
                .filter(|gap| *gap > 0.0)
                .collect::<Vec<_>>()
        })
        .collect();
    gaps.sort_by(f64::total_cmp);
    let typical = gaps.get(gaps.len() / 2).copied().unwrap_or(1.0);
    Ok((spacing / typical, Scale::PixelSpacing(spacing)))
}

/// The Donder controllers, with each xLights output's port.
struct Controllers {
    controllers: Vec<(Identifier, Controller)>,
    /// By xLights controller and output.
    ports: HashMap<(usize, usize), (ControllerId, ControllerPortId, u32)>,
    skipped: Vec<SkippedController>,
}

/// The Donder protocol of an xLights controller, or why it has none.
fn protocol(
    controller: &crate::networks::NetworkController,
) -> Result<ControllerProtocol, ControllerSkip> {
    match controller.kind {
        ControllerType::Serial => return Err(ControllerSkip::Serial),
        ControllerType::Null => return Err(ControllerSkip::Null),
        ControllerType::Ethernet => {}
    }
    if controller.player_only {
        return Err(ControllerSkip::PlayerOnly);
    }
    let kind = controller
        .protocol
        .ok_or_else(|| ControllerSkip::UnknownProtocol(controller.protocol_name.clone()))?;
    if !matches!(kind, OutputType::E131 | OutputType::ArtNet) {
        return Err(ControllerSkip::Protocol(kind.name()));
    }
    if !controller.active {
        return Err(ControllerSkip::Inactive);
    }
    let address = || {
        controller
            .ip
            .parse::<IpAddr>()
            .map_err(|_| ControllerSkip::Address(controller.ip.clone()))
    };
    let mut universes = HashSet::new();
    for output in &controller.outputs {
        if output.kind != kind {
            return Err(ControllerSkip::MixedOutputs);
        }
        if output.channels == 0 {
            return Err(ControllerSkip::EmptyOutput);
        }
        let range = if kind == OutputType::E131 {
            1..=63_999
        } else {
            0..=32_767
        };
        if !range.contains(&output.universe) {
            return Err(ControllerSkip::Universe(output.universe));
        }
        if !universes.insert(output.universe) {
            return Err(ControllerSkip::RepeatedUniverse(output.universe));
        }
    }
    Ok(match kind {
        OutputType::E131 => {
            let mode = if controller.ip.eq_ignore_ascii_case("MULTICAST") {
                E131Mode::Multicast
            } else {
                E131Mode::Unicast {
                    destination: address()?,
                }
            };
            let priority = u8::try_from(controller.priority)
                .ok()
                .filter(|priority| (1..=200).contains(priority))
                .ok_or(ControllerSkip::Priority(controller.priority))?;
            ControllerProtocol::E131(E131Config {
                sender_name: "Donder".to_string(),
                bind_address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                priority,
                mode,
            })
        }
        _ => {
            let destination = address()?;
            ControllerProtocol::ArtNet(ArtNetConfig {
                bind_address: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 6454),
                destination: SocketAddr::new(destination, 6454),
                mode: ArtNetMode::Unicast,
            })
        }
    })
}

fn controllers(networks: &Networks, mut new_id: impl FnMut() -> u32) -> Controllers {
    let mut result = Controllers {
        controllers: Vec::new(),
        ports: HashMap::new(),
        skipped: Vec::new(),
    };
    let mut taken = HashSet::new();
    for (index, controller) in networks.controllers.iter().enumerate() {
        let protocol = match protocol(controller) {
            Ok(protocol) => protocol,
            Err(reason) => {
                result.skipped.push(SkippedController {
                    controller: controller.name.clone(),
                    reason,
                });
                continue;
            }
        };
        let id = ControllerId(new_id());
        let mut ports = Vec::new();
        for (output_index, output) in controller.outputs.iter().enumerate() {
            let universe = output.universe as u16;
            let address = match protocol {
                ControllerProtocol::ArtNet(_) => ControllerPortAddress::ArtNetPort(universe),
                _ => ControllerPortAddress::E131Universe(universe),
            };
            let port = ControllerPortId(ports.len() as u32 + 1);
            ports.push(ControllerPort {
                id: port,
                name: donder_language::object_name(&format!("universe_{}", output.universe)),
                address,
                slot_count: output.channels as u16,
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
    result
}

/// Slots from `0` up to `1` of an output, patched to the model named `2`.
type Claim = (u32, u32, String);

/// A model to import: its pixels in channel order.
struct Imported {
    name: String,
    description: Option<String>,
    pixels: Vec<Pixel>,
    ellipses: Vec<models::Ellipse>,
    /// Each pixel's first absolute channel, when the model is patched.
    channels: Option<Vec<u32>>,
    encoding: PixelEncoding,
    dimming: Option<Dimming>,
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
            return Err(UnroutedReason::UnsupportedController(
                networks.controllers[slot.controller].name.clone(),
            ));
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
    let root = read_file(&show.join(LAYOUT_FILE))?;
    let networks_path = show.join(NETWORKS_FILE);
    let networks = if networks_path.exists() {
        Networks::read(&read_file(&networks_path)?)
    } else {
        Networks::default()
    };
    let mut report = ImportReport::default();
    let (models, unloaded) = read_models(&root, &networks);
    report.skipped.extend(unloaded);
    // Only placed models' pixels set the typical spacing: positions() is
    // empty for the rest.
    let (meters, scale) = scale(&root, &models, options)?;
    report.meters_per_unit = meters;
    report.scale = Some(scale);

    let mut imported = Vec::new();
    for model in &models {
        let local = match &model.local {
            Ok(local) => local,
            Err(reason) => {
                report.skipped.push(Skipped {
                    model: model.name.clone(),
                    reason: reason.clone(),
                });
                continue;
            }
        };
        if local.nodes.is_empty() {
            report.skipped.push(Skipped {
                model: model.name.clone(),
                reason: SkipReason::NoPixels,
            });
            continue;
        }
        let Some(encoding) = model.encoding else {
            continue;
        };
        // A fixture's pixels go in channel order, so its routes are runs of
        // consecutive channels.
        let mut order: Vec<usize> = (0..local.nodes.len()).collect();
        order.sort_by_key(|&index| local.nodes[index].channel);
        let channels = match &model.start {
            Ok(start) => order
                .iter()
                .map(|&index| start.checked_add(local.nodes[index].channel))
                .collect::<Option<Vec<u32>>>()
                .ok_or_else(|| UnroutedReason::OutOfRange(model.start_channel.clone())),
            Err(reason) => Err(reason.clone()),
        };
        let channels = match channels {
            Ok(channels) => Some(channels),
            Err(reason) => {
                report.unrouted.push(Unrouted {
                    model: model.name.clone(),
                    reason,
                });
                None
            }
        };
        let transform = local.placement.transform(local.size);
        let to_meters = |point: DVec3| point * meters;
        let positions = model.positions();
        imported.push(Imported {
            name: model.name.clone(),
            description: model.description.clone(),
            pixels: order
                .iter()
                .map(|&index| Pixel {
                    position: to_meters(positions[index]),
                    arc: local.nodes[index].arc.map(|arc| (arc.arc, arc.degrees)),
                })
                .collect(),
            ellipses: local
                .ellipses
                .iter()
                .map(|ellipse| {
                    let point = |p: DVec3| {
                        let p = transform.transform_point3(p.as_vec3());
                        to_meters(p.as_dvec3())
                    };
                    let vector =
                        |v: DVec3| to_meters(transform.transform_vector3(v.as_vec3()).as_dvec3());
                    models::Ellipse {
                        center: point(ellipse.center),
                        u: vector(ellipse.u),
                        v: vector(ellipse.v),
                    }
                })
                .collect(),
            channels,
            encoding,
            dimming: model.dimming,
        });
    }

    let controllers = controllers(&networks, || session.project.new_id());
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
        let (brightness, gamma) = model
            .dimming
            .map_or((1.0, 1.0), |dimming| (dimming.brightness, dimming.gamma));
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
                gamma,
                brightness,
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
