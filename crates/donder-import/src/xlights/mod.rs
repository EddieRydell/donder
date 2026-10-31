//! An xLights show folder: `xlights_rgbeffects.xml` holds the layout and
//! `xlights_networks.xml` the controllers. Each model becomes a fixture whose
//! pixels sit where xLights draws its nodes, in channel order, and its
//! channels become routes on the controller ports that carry them.

mod channels;
mod kind;
pub(crate) mod models;
pub(crate) mod networks;
mod placement;
pub(crate) mod read;
mod submodels;

use crate::fit::{Ellipse, Pixel};
use crate::report::{
    ControllerSkip, ImportOptions, ImportReport, Scale, SkipReason, Skipped, TYPICAL_PIXEL_SPACING,
    Unrouted, UnroutedReason,
};
use crate::setup::{
    ImportError, Profile, ProfileController, ProfileFixture, ProfileGroup, ProfileOutput,
};
use crate::xml::{self, Element};
use camino::Utf8Path;
use channels::ModelChannels;
use donder_model::{ArtNetConfig, ArtNetMode, ControllerProtocol, E131Config, E131Mode};
use donder_runtime_types::PixelEncoding;
use glam::{DVec3, Vec3};
use kind::ModelType;
use models::{Dimming, Local};
use networks::{ControllerType, Networks, OutputType};
use read::Reader;
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub(crate) const LAYOUT_FILE: &str = "xlights_rgbeffects.xml";
pub(crate) const NETWORKS_FILE: &str = "xlights_networks.xml";

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
    /// Its submodels, when it is laid out.
    pub(crate) submodels: Vec<submodels::SubModel>,
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
    let mut read_elements = Vec::new();
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
        read_elements.push(element);
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
                start: networks.start_channel(model.common.start_channel),
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
        .zip(read_elements)
        .map(|((model, (status, start)), element)| ShowModel {
            submodels: model.local.as_ref().map_or_else(
                |_| Vec::new(),
                |local| submodels::read(&model.common.name, element, local.nodes.len()),
            ),
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
    let gaps = models.iter().flat_map(|model| {
        model
            .positions()
            .windows(2)
            .map(|pair| pair[0].distance(pair[1]))
            .collect::<Vec<_>>()
    });
    Ok((
        crate::setup::typical_scale(gaps, spacing),
        Scale::PixelSpacing(spacing),
    ))
}

/// The Donder protocol of an xLights controller, or why it has none.
fn protocol(
    controller: &crate::xlights::networks::NetworkController,
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
        if output.channels > 512 {
            return Err(ControllerSkip::LargeOutput(output.channels));
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

/// The show's controllers, by the index slots refer to.
fn controllers(networks: &Networks) -> Vec<ProfileController> {
    networks
        .controllers
        .iter()
        .map(|controller| {
            let protocol = protocol(controller);
            // protocol() has checked each output's universe and channels.
            let outputs = match &protocol {
                Ok(_) => controller
                    .outputs
                    .iter()
                    .map(|output| ProfileOutput {
                        universe: output.universe as u16,
                        channels: output.channels as u16,
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };
            ProfileController {
                name: controller.name.clone(),
                description: controller.description.clone(),
                protocol,
                outputs,
            }
        })
        .collect()
}

/// The groups of the layout. xLights: `ModelGroup`, whose `models` lists
/// models, groups and submodels by name.
fn groups(root: &Element) -> Vec<ProfileGroup> {
    root.child("modelGroups")
        .into_iter()
        .flat_map(|groups| groups.children("modelGroup"))
        .map(|group| {
            let mut seen = HashSet::new();
            let members = group
                .attribute("models")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty() && seen.insert(*entry))
                .map(str::to_string)
                .collect();
            ProfileGroup {
                name: group
                    .attribute("name")
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                members,
            }
        })
        .collect()
}

pub(crate) fn read(folder: &Utf8Path, options: &ImportOptions) -> Result<Profile, ImportError> {
    let root = xml::read_file(&folder.join(LAYOUT_FILE))?;
    let networks_path = folder.join(NETWORKS_FILE);
    let networks = if networks_path.exists() {
        Networks::read(&xml::read_file(&networks_path)?)
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

    let groups = groups(&root);
    // Names a split model's unnamed cells must not take.
    let mut taken: HashSet<String> = models
        .iter()
        .flat_map(|model| {
            std::iter::once(model.name.clone())
                .chain(model.submodels.iter().map(|submodel| submodel.name.clone()))
        })
        .chain(groups.iter().map(|group| group.name.clone()))
        .collect();
    let mut part_groups = Vec::new();
    let mut fixtures = Vec::new();
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
        let slots = match &model.start {
            Ok(start) => order
                .iter()
                .map(|&index| start.checked_add(local.nodes[index].channel))
                .collect::<Option<Vec<u32>>>()
                .ok_or_else(|| UnroutedReason::OutOfRange(model.start_channel.clone()))
                .and_then(|channels| {
                    channels
                        .iter()
                        .map(|&channel| networks.slot(channel))
                        .collect::<Option<Vec<_>>>()
                        .ok_or(UnroutedReason::OutsideOutputs)
                }),
            Err(reason) => Err(reason.clone()),
        };
        let slots = match slots {
            Ok(slots) => Some(slots),
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
        let fixture = ProfileFixture {
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
                    Ellipse {
                        center: point(ellipse.center),
                        u: vector(ellipse.u),
                        v: vector(ellipse.v),
                    }
                })
                .collect(),
            encoding,
            dimming: model.dimming,
            slots,
        };
        split(
            fixture,
            &model.submodels,
            &order,
            &mut taken,
            &mut fixtures,
            &mut part_groups,
            &mut report,
        );
    }
    Ok(Profile {
        fixtures,
        controllers: controllers(&networks),
        groups: groups.into_iter().chain(part_groups).collect(),
        report,
    })
}

/// A model split into the fixtures its submodels need: its pixels in the
/// same set of submodels are one fixture, so each submodel is a group of
/// whole fixtures. A cell that is exactly a submodel takes its name; the
/// model, and a submodel of several cells or one named otherwise, become
/// groups of their cells. `order` holds each pixel's node.
fn split(
    fixture: ProfileFixture,
    submodels: &[submodels::SubModel],
    order: &[usize],
    taken: &mut HashSet<String>,
    fixtures: &mut Vec<ProfileFixture>,
    groups: &mut Vec<ProfileGroup>,
    report: &mut ImportReport,
) {
    let mut pixel_of = vec![0; order.len()];
    for (pixel, &node) in order.iter().enumerate() {
        pixel_of[node] = pixel;
    }
    let mut parts: Vec<(&str, HashSet<usize>)> = Vec::new();
    for submodel in submodels {
        match &submodel.nodes {
            Ok(nodes) if nodes.is_empty() => report.skipped.push(Skipped {
                model: submodel.name.clone(),
                reason: SkipReason::NoPixels,
            }),
            Ok(nodes) => parts.push((
                submodel.name.as_str(),
                nodes.iter().map(|&node| pixel_of[node]).collect(),
            )),
            Err(reason) => report.skipped.push(Skipped {
                model: submodel.name.clone(),
                reason: reason.clone(),
            }),
        }
    }
    let sets: Vec<HashSet<usize>> = parts.iter().map(|(_, pixels)| pixels.clone()).collect();
    let cells = crate::setup::cells(fixture.pixels.len(), &sets);
    let alias = |name: &str, members: Vec<String>| ProfileGroup {
        name: name.to_string(),
        members,
    };
    if let [_] = cells[..] {
        for (name, _) in &parts {
            groups.push(alias(name, vec![fixture.name.clone()]));
        }
        fixtures.push(fixture);
        return;
    }
    // Each cell's name: the first submodel that is exactly it, else the
    // model's name and its number.
    let names: Vec<String> = cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let exact = parts.iter().find(|(_, pixels)| {
                pixels.len() == cell.len() && cell.iter().all(|p| pixels.contains(p))
            });
            match exact {
                Some((name, _)) => name.to_string(),
                None => crate::setup::unique(format!("{} {}", fixture.name, index + 1), taken),
            }
        })
        .collect();
    for (name, pixels) in &parts {
        let members: Vec<String> = cells
            .iter()
            .zip(&names)
            .filter(|(cell, _)| pixels.contains(&cell[0]))
            .map(|(_, cell_name)| cell_name.clone())
            .collect();
        if !matches!(&members[..], [only] if only == name) {
            groups.push(alias(name, members));
        }
    }
    groups.push(alias(&fixture.name, names.clone()));
    for (cell, name) in cells.iter().zip(names) {
        fixtures.push(fixture.subset(name, cell));
    }
}
