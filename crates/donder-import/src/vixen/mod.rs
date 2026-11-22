//! A Vixen 3 profile folder: `SystemData/SystemConfig.xml` holds the
//! elements, controllers and patching, and `SystemData/ModuleStore.xml` each
//! module's settings, including the preview's shapes. Each shape of the
//! enabled preview becomes the fixtures its wiring makes, whose pixels sit
//! where Vixen draws them, in channel order, and each pixel's element is
//! patched to the controller outputs its color breakdown feeds. The element
//! tree's groups become layout groups of fixtures and slices of them.

mod config;
pub(crate) mod contract;
mod controllers;
mod patching;
pub(crate) mod shapes;

use crate::fit::{Ellipse, Pixel};
use crate::report::{
    FittedDimming, GroupSkip, ImportOptions, ImportReport, SkipReason, Skipped, SkippedGroup,
    Unrouted, UnroutedReason,
};
use crate::setup::{
    Dimming, ImportError, Profile, ProfileController, ProfileFixture, ProfileGroup, ProfileMember,
    Slot, unique,
};
use crate::xml;
use camino::Utf8Path;
use config::Config;
use donder_runtime_types::PixelEncoding;
use glam::{DVec2, DVec3};
use patching::{Breakdown, COLOR_BREAKDOWN, Curve, DIMMING_CURVE, PatchedPixel, Patching};
use shapes::Laid;
use std::collections::{HashMap, HashSet};

pub(crate) const SYSTEM_CONFIG_FILE: &str = "SystemData/SystemConfig.xml";
pub(crate) const MODULE_STORE_FILE: &str = "SystemData/ModuleStore.xml";

/// The preview whose shapes the importer places: the one enabled, or the
/// one named.
fn preview<'a>(
    config: &'a Config,
    options: &ImportOptions,
) -> Result<&'a config::ConfigPreview, ImportError> {
    if let Some(name) = &options.preview {
        return config
            .previews
            .iter()
            .find(|preview| &preview.name == name)
            .ok_or_else(|| ImportError::UnknownPreview(name.clone()));
    }
    let enabled: Vec<&config::ConfigPreview> = config
        .previews
        .iter()
        .filter(|preview| !config.disabled.contains(&preview.instance_id))
        .collect();
    match enabled[..] {
        [preview] => Ok(preview),
        [] => Err(ImportError::NoPreview),
        _ => Err(ImportError::SeveralPreviews(
            enabled.iter().map(|preview| preview.name.clone()).collect(),
        )),
    }
}

/// Each module's settings, by module instance, for the instances wanted.
/// The module store lists module type data, then instance data; an
/// instance's settings are the element inside its `Module`.
fn modules(
    path: &Utf8Path,
    wanted: &HashSet<String>,
) -> Result<HashMap<String, xml::Element>, ImportError> {
    let kept = xml::read_kept(path, |element| {
        element.name == "Module"
            && element
                .attribute("moduleInstance")
                .is_some_and(|instance| wanted.contains(&instance.trim().to_ascii_lowercase()))
    })?;
    Ok(kept
        .into_iter()
        .filter_map(|mut module| {
            let instance = module
                .attribute("moduleInstance")?
                .trim()
                .to_ascii_lowercase();
            let data = module.children.drain(..).next()?;
            Some((instance, data))
        })
        .collect())
}

/// A shape's own name, else its type; a fixture of the shape takes the name
/// of the group of exactly its imported elements instead, when there is one.
fn shape_name(shape: &xml::Element) -> String {
    if let Some(name) = contract::text(shape, "Name").filter(|name| !name.trim().is_empty()) {
        return name.trim().to_string();
    }
    let kind = contract::type_name(shape).unwrap_or("Shape");
    kind.strip_prefix("Preview").unwrap_or(kind).to_string()
}

/// Each group node's leaves, by node id.
fn leaves(config: &Config) -> HashMap<&str, HashSet<&str>> {
    fn collect<'a>(
        config: &'a Config,
        node: &'a str,
        memo: &mut HashMap<&'a str, HashSet<&'a str>>,
        open: &mut HashSet<&'a str>,
    ) -> HashSet<&'a str> {
        if let Some(leaves) = memo.get(node) {
            return leaves.clone();
        }
        let Some(entry) = config.nodes.get(node) else {
            return HashSet::new();
        };
        if entry.channel.is_some() {
            return HashSet::from([node]);
        }
        // A node under itself: Vixen's tree has no loops, so this only
        // guards a malformed file from recursing forever.
        if !open.insert(node) {
            return HashSet::new();
        }
        let mut leaves = HashSet::new();
        for child in &entry.children {
            leaves.extend(collect(config, child, memo, open));
        }
        open.remove(node);
        memo.insert(node, leaves.clone());
        leaves
    }
    let mut memo = HashMap::new();
    for node in &config.node_order {
        collect(config, node, &mut memo, &mut HashSet::new());
    }
    memo
}

/// A laid-out shape, its pixels linked to elements.
struct Placed<'a> {
    name: String,
    laid: Laid,
    /// Each pixel's element node, for the pixels linked to one.
    nodes: Vec<&'a str>,
}

/// The group whose leaves are exactly a set of nodes, sorted, the first in
/// the tree with them.
fn exact_groups<'a>(
    config: &'a Config,
    leaves: &HashMap<&'a str, HashSet<&'a str>>,
) -> HashMap<Vec<&'a str>, &'a str> {
    let mut exact = HashMap::new();
    for node in &config.node_order {
        if let Some(set) = leaves.get(node.as_str()) {
            let mut key: Vec<&str> = set.iter().copied().collect();
            key.sort_unstable();
            exact.entry(key).or_insert(node.as_str());
        }
    }
    exact
}

/// Whether pixel `next` takes the channels right after pixel `previous`: on
/// the same output, or at the start of the output of the next universe
/// once `previous` fills its own.
fn follows(previous: Slot, next: Slot, width: u32, controllers: &[ProfileController]) -> bool {
    if previous.controller != next.controller {
        return false;
    }
    if previous.output == next.output {
        return next.offset == previous.offset + width;
    }
    let outputs = &controllers[previous.controller].outputs;
    let (Some(from), Some(to)) = (outputs.get(previous.output), outputs.get(next.output)) else {
        return false;
    };
    next.offset == 0
        && previous.offset + width == u32::from(from.channels)
        && u32::from(to.universe) == u32::from(from.universe) + 1
}

/// A fixture before it is named: its pixels and their elements in its
/// pixel order, its patching, and the shape it is part of.
struct Wired<'a> {
    shape: String,
    laid: Laid,
    nodes: Vec<&'a str>,
    patch: Result<(Vec<Slot>, PixelEncoding, Option<Dimming>), UnroutedReason>,
}

/// Each shape as the fixtures its wiring makes: a fixture is a run of the
/// shape's pixels, in channel order, that take consecutive channels of one
/// controller with one color order and dimming, so it is one natural
/// patching unit. A shape whose pixels are not all patched stays one
/// fixture in its own order. The element groups within a fixture become
/// layout groups of its slices.
fn wire<'a>(
    placed: Vec<Placed<'a>>,
    config: &'a Config,
    patching: &Patching<'_>,
    controllers: &[ProfileController],
) -> Vec<Wired<'a>> {
    let mut wired = Vec::new();
    for shape in placed {
        let patches = shape
            .nodes
            .iter()
            .map(|node| {
                let channel = config.nodes[*node].channel.as_deref().unwrap_or_default();
                patching
                    .pixel(channel)
                    .map_err(|problem| UnroutedReason::Element {
                        element: config.nodes[*node].name.clone(),
                        problem,
                    })
            })
            .collect::<Result<Vec<PatchedPixel>, _>>()
            .and_then(|patches| {
                if patches
                    .windows(2)
                    .all(|pair| pair[0].encoding == pair[1].encoding)
                {
                    Ok(patches)
                } else {
                    Err(UnroutedReason::MixedEncodings)
                }
            });
        let patches = match patches {
            Ok(patches) => patches,
            Err(reason) => {
                wired.push(Wired {
                    shape: shape.name,
                    laid: shape.laid,
                    nodes: shape.nodes,
                    patch: Err(reason),
                });
                continue;
            }
        };
        let width = patches
            .first()
            .map_or(3, |patch| patch.encoding.channel_order().len() as u32);
        let mut order: Vec<usize> = (0..patches.len()).collect();
        order.sort_by_key(|&index| {
            let slot = patches[index].slot;
            (slot.controller, slot.output, slot.offset)
        });
        let mut runs: Vec<Vec<usize>> = Vec::new();
        for index in order {
            match runs.last_mut() {
                Some(run)
                    if run.last().is_some_and(|&last| {
                        follows(patches[last].slot, patches[index].slot, width, controllers)
                            && patches[last].dimming == patches[index].dimming
                    }) =>
                {
                    run.push(index);
                }
                _ => runs.push(vec![index]),
            }
        }
        for run in runs {
            let first = patches[run[0]];
            wired.push(Wired {
                shape: shape.name.clone(),
                laid: Laid {
                    pixels: run
                        .iter()
                        .map(|&index| shape.laid.pixels[index].clone())
                        .collect(),
                    ellipses: shape.laid.ellipses.clone(),
                },
                nodes: run.iter().map(|&index| shape.nodes[index]).collect(),
                patch: Ok((
                    run.iter().map(|&index| patches[index].slot).collect(),
                    first.encoding,
                    first.dimming,
                )),
            });
        }
    }
    wired
}

/// Each fixture's name: the group of exactly its elements, else its shape's,
/// numbered when its shape makes several fixtures. Group names are claimed
/// first, so a shape's name never takes a group's.
fn names(
    wired: &[Wired<'_>],
    config: &Config,
    exact: &HashMap<Vec<&str>, &str>,
    taken: &mut HashSet<String>,
) -> Vec<String> {
    let mut parts: HashMap<&str, usize> = HashMap::new();
    for fixture in wired {
        *parts.entry(fixture.shape.as_str()).or_default() += 1;
    }
    let groups: Vec<Option<&str>> = wired
        .iter()
        .map(|fixture| {
            let mut key = fixture.nodes.clone();
            key.sort_unstable();
            exact
                .get(&key)
                .and_then(|group| config.nodes.get(*group))
                .map(|group| group.name.as_str())
        })
        .collect();
    let mut names: Vec<Option<String>> = groups
        .iter()
        .map(|group| group.map(|group| unique(group.to_string(), taken)))
        .collect();
    let mut numbers: HashMap<&str, usize> = HashMap::new();
    for (fixture, name) in wired.iter().zip(&mut names) {
        let number = numbers.entry(fixture.shape.as_str()).or_default();
        *number += 1;
        if name.is_none() {
            let base = if parts[fixture.shape.as_str()] == 1 {
                fixture.shape.clone()
            } else {
                format!("{} {}", fixture.shape, number)
            };
            *name = Some(unique(base, taken));
        }
    }
    names
        .into_iter()
        .zip(wired)
        .map(|(name, fixture)| name.unwrap_or_else(|| fixture.shape.clone()))
        .collect()
}

pub(crate) fn read(folder: &Utf8Path, options: &ImportOptions) -> Result<Profile, ImportError> {
    let config_path = folder.join(SYSTEM_CONFIG_FILE);
    let config =
        Config::read(&xml::read_file(&config_path)?).map_err(|message| ImportError::Xml {
            path: config_path.clone(),
            message,
        })?;
    let preview = preview(&config, options)?;
    let filters_of = |kind: &'static str| {
        config
            .filters
            .iter()
            .filter(move |(_, filter)| filter.as_str() == kind)
            .map(|(instance, _)| instance.clone())
    };
    let mut wanted: HashSet<String> = filters_of(COLOR_BREAKDOWN).collect();
    wanted.extend(filters_of(DIMMING_CURVE));
    wanted.extend(
        config
            .controllers
            .iter()
            .map(|controller| controller.hardware_instance_id.clone()),
    );
    wanted.insert(preview.instance_id.clone());
    let modules = modules(&folder.join(MODULE_STORE_FILE), &wanted)?;
    let mut report = ImportReport::default();

    let controllers = controllers::read(&config.controllers, &modules, &config.disabled);
    let breakdowns: HashMap<String, Result<Breakdown, contract::FieldError>> =
        filters_of(COLOR_BREAKDOWN)
            .filter_map(|instance| {
                let breakdown = Breakdown::read(modules.get(&instance)?);
                Some((instance, breakdown))
            })
            .collect();
    let curves: HashMap<String, Result<Curve, contract::FieldError>> = filters_of(DIMMING_CURVE)
        .filter_map(|instance| {
            let curve = Curve::read(modules.get(&instance)?);
            Some((instance, curve))
        })
        .collect();
    let patching = Patching::new(&config, &breakdowns, &curves, &controllers.slots);

    let leaves = leaves(&config);

    let shapes_list: Vec<&xml::Element> = modules
        .get(&preview.instance_id)
        .and_then(|data| contract::field(data, "DisplayItems"))
        .map(|items| {
            items
                .children("DisplayItem")
                .filter_map(|item| item.child("Shape"))
                .collect()
        })
        .unwrap_or_default();

    let mut taken = HashSet::new();
    let mut placed: Vec<Placed> = Vec::new();
    let mut lit: HashSet<&str> = HashSet::new();
    for shape in shapes_list {
        let laid = match shapes::layout(shape) {
            Ok(laid) => laid,
            Err(reason) => {
                report.skipped.push(Skipped {
                    model: shape_name(shape),
                    reason: SkipReason::Shape(reason),
                });
                continue;
            }
        };
        // Pixels linked to no element, or to one the tree does not hold,
        // light nothing.
        let mut pixels = Vec::new();
        let mut nodes = Vec::new();
        for pixel in laid.pixels {
            let node = pixel.node.as_deref().and_then(|node| {
                config
                    .nodes
                    .get_key_value(node)
                    .filter(|(_, entry)| entry.channel.is_some())
                    .map(|(key, _)| key.as_str())
            });
            if let Some(node) = node {
                nodes.push(node);
                pixels.push(pixel);
            }
        }
        let name = shape_name(shape);
        if nodes.is_empty() {
            report.skipped.push(Skipped {
                model: name,
                reason: SkipReason::Unlinked,
            });
            continue;
        }
        let mut seen = HashSet::new();
        let twice = nodes
            .iter()
            .find(|node| lit.contains(*node) || !seen.insert(**node));
        if let Some(node) = twice {
            report.skipped.push(Skipped {
                model: name,
                reason: SkipReason::LitTwice(config.nodes[*node].name.clone()),
            });
            continue;
        }
        lit.extend(nodes.iter().copied());
        placed.push(Placed {
            name,
            laid: Laid {
                pixels,
                ellipses: laid.ellipses,
            },
            nodes,
        });
    }
    // Elements in no shape are not imported, so each group from here on is
    // its imported elements.
    let leaves: HashMap<&str, HashSet<&str>> = leaves
        .into_iter()
        .map(|(group, set)| {
            let set: HashSet<&str> = set.into_iter().filter(|leaf| lit.contains(leaf)).collect();
            (group, set)
        })
        .filter(|(_, set)| !set.is_empty())
        .collect();
    let exact = exact_groups(&config, &leaves);
    let wired = wire(placed, &config, &patching, &controllers.controllers);
    let names = names(&wired, &config, &exact, &mut taken);

    let (meters, scale) = crate::setup::typical_scale(
        wired.iter().map(|fixture| {
            fixture
                .laid
                .pixels
                .iter()
                .map(|pixel| pixel.position.extend(0.0))
                .collect()
        }),
        options.pixel_spacing,
    );
    report.meters_per_unit = meters;
    report.scale = Some(scale);
    // Preview units run down the screen; layout y runs up.
    let place = |point: DVec2| DVec3::new(point.x, -point.y, 0.0) * meters;
    let vector = |vector: DVec2| DVec3::new(vector.x, -vector.y, 0.0) * meters;

    let mut fixtures = Vec::new();
    for (fixture, name) in wired.iter().zip(&names) {
        let (slots, encoding, dimming) = match &fixture.patch {
            Ok((slots, encoding, dimming)) => (Some(slots.clone()), *encoding, *dimming),
            Err(reason) => {
                report.unrouted.push(Unrouted {
                    model: name.clone(),
                    reason: reason.clone(),
                });
                // An unpatched fixture has no routes for its order to apply to.
                (None, PixelEncoding::Rgb { order: [0, 1, 2] }, None)
            }
        };
        if let Some(dimming) = dimming {
            report.fitted_dimming.push(FittedDimming {
                model: name.clone(),
                brightness: dimming.brightness,
                gamma: dimming.gamma,
            });
        }
        fixtures.push(ProfileFixture {
            name: name.clone(),
            description: None,
            pixels: fixture
                .laid
                .pixels
                .iter()
                .map(|pixel| Pixel {
                    position: place(pixel.position),
                    arc: pixel.arc,
                })
                .collect(),
            ellipses: fixture
                .laid
                .ellipses
                .iter()
                .map(|ellipse| Ellipse {
                    center: place(ellipse.center),
                    u: vector(ellipse.u),
                    v: vector(ellipse.v),
                })
                .collect(),
            encoding,
            dimming,
            slots,
        });
    }

    let groups = groups(&config, &leaves, &wired, &names, &mut taken, &mut report);
    Ok(Profile {
        fixtures,
        controllers: controllers.controllers,
        groups,
        report,
    })
}

/// The element tree's groups, children before the groups holding them. A
/// group holds the groups under it that are imported, and the pixels of
/// its other elements: a whole fixture as that fixture, part of one as its
/// slices.
struct GroupWalk<'a> {
    config: &'a Config,
    leaves: &'a HashMap<&'a str, HashSet<&'a str>>,
    names: &'a [String],
    /// Each fixture's pixel count.
    sizes: Vec<usize>,
    /// The fixture each element is in, and its pixel there.
    pixel_of: HashMap<&'a str, (usize, usize)>,
    /// Each imported group's name, by node id.
    imported: HashMap<&'a str, String>,
    visited: HashSet<&'a str>,
    groups: Vec<ProfileGroup>,
}

impl<'a> GroupWalk<'a> {
    /// The pixels of `leaves`, in fixture and pixel order.
    fn pixels(&self, leaves: impl IntoIterator<Item = &'a str>) -> Vec<(usize, usize)> {
        let mut pixels: Vec<(usize, usize)> = leaves
            .into_iter()
            .filter_map(|leaf| self.pixel_of.get(leaf).copied())
            .collect();
        pixels.sort_unstable();
        pixels
    }

    fn child_leaves(&self, child: &'a str) -> Vec<&'a str> {
        match self.leaves.get(child) {
            Some(leaves) => leaves.iter().copied().collect(),
            None => vec![child],
        }
    }

    /// `pending` pixels as members: each fixture, in the order they come,
    /// whole or as the runs of its pixels in fixture order.
    fn flush(&self, pending: &mut Vec<(usize, usize)>, members: &mut Vec<ProfileMember>) {
        let mut fixtures: Vec<usize> = Vec::new();
        for &(fixture, _) in pending.iter() {
            if !fixtures.contains(&fixture) {
                fixtures.push(fixture);
            }
        }
        for fixture in fixtures {
            let mut pixels: Vec<usize> = pending
                .iter()
                .filter(|(owner, _)| *owner == fixture)
                .map(|(_, pixel)| *pixel)
                .collect();
            pixels.sort_unstable();
            pixels.dedup();
            let name = &self.names[fixture];
            let mut push = |member: ProfileMember| {
                if !members.contains(&member) {
                    members.push(member);
                }
            };
            if pixels.len() == self.sizes[fixture] {
                push(ProfileMember::Named(name.clone()));
                continue;
            }
            let mut start = 0;
            while start < pixels.len() {
                let mut end = start + 1;
                while end < pixels.len() && pixels[end] == pixels[end - 1] + 1 {
                    end += 1;
                }
                push(ProfileMember::Slice {
                    fixture: name.clone(),
                    start: pixels[start] as u32,
                    count: (end - start) as u32,
                });
                start = end;
            }
        }
        pending.clear();
    }

    fn visit(&mut self, node: &'a str, taken: &mut HashSet<String>, report: &mut ImportReport) {
        let config = self.config;
        let Some(entry) = config.nodes.get(node) else {
            return;
        };
        if entry.channel.is_some() || !self.visited.insert(node) {
            return;
        }
        for child in &entry.children {
            self.visit(child, taken, report);
        }
        let skip = |reason| SkippedGroup {
            group: entry.name.clone(),
            reason,
        };
        let Some(group_leaves) = self.leaves.get(node) else {
            report.skipped_groups.push(skip(GroupSkip::Empty));
            return;
        };
        let held = self.pixels(group_leaves.iter().copied());
        if held.is_empty() {
            report.skipped_groups.push(skip(GroupSkip::Empty));
            return;
        }
        // A group of exactly one fixture's elements is another name for
        // that fixture, which already takes the first such group's name.
        if let Some(&(only, _)) = held.first()
            && held.iter().all(|(fixture, _)| *fixture == only)
            && held.len() == self.sizes[only]
        {
            return;
        }
        // Members in the order of the group's children: an imported child
        // group, or the pixels of the elements of other children, each
        // element once.
        let mut members = Vec::new();
        let mut pending = Vec::new();
        let mut listed: HashSet<&str> = HashSet::new();
        for child in &entry.children {
            let leaves = self.child_leaves(child);
            if let Some(name) = self.imported.get(child.as_str()) {
                self.flush(&mut pending, &mut members);
                members.push(ProfileMember::Named(name.clone()));
                listed.extend(leaves);
                continue;
            }
            let fresh: Vec<&str> = leaves
                .into_iter()
                .filter(|leaf| listed.insert(leaf))
                .collect();
            pending.extend(self.pixels(fresh));
        }
        self.flush(&mut pending, &mut members);
        let name = unique(entry.name.clone(), taken);
        self.imported.insert(node, name.clone());
        self.groups.push(ProfileGroup { name, members });
    }
}

fn groups<'a>(
    config: &'a Config,
    leaves: &'a HashMap<&'a str, HashSet<&'a str>>,
    wired: &'a [Wired<'a>],
    names: &'a [String],
    taken: &mut HashSet<String>,
    report: &mut ImportReport,
) -> Vec<ProfileGroup> {
    let mut walk = GroupWalk {
        config,
        leaves,
        names,
        sizes: wired.iter().map(|fixture| fixture.nodes.len()).collect(),
        pixel_of: wired
            .iter()
            .enumerate()
            .flat_map(|(index, fixture)| {
                fixture
                    .nodes
                    .iter()
                    .enumerate()
                    .map(move |(pixel, node)| (*node, (index, pixel)))
            })
            .collect(),
        imported: HashMap::new(),
        visited: HashSet::new(),
        groups: Vec::new(),
    };
    for root in &config.roots {
        walk.visit(root, taken, report);
    }
    walk.groups
}
