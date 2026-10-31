//! A Vixen 3 profile folder: `SystemData/SystemConfig.xml` holds the
//! elements, controllers and patching, and `SystemData/ModuleStore.xml` each
//! module's settings, including the preview's shapes. Each shape of the
//! enabled preview becomes a fixture whose pixels sit where Vixen draws
//! them, in channel order, and each pixel's element is patched to the
//! controller outputs its color breakdown feeds.

mod config;
pub(crate) mod contract;
mod controllers;
mod patching;
pub(crate) mod shapes;

use crate::fit::{Ellipse, Pixel};
use crate::report::{
    GroupSkip, ImportOptions, ImportReport, Scale, SkipReason, Skipped, SkippedGroup,
    TYPICAL_PIXEL_SPACING, Unrouted, UnroutedReason,
};
use crate::setup::{ImportError, Profile, ProfileFixture, ProfileGroup, Slot, unique};
use crate::xml;
use camino::Utf8Path;
use config::Config;
use donder_runtime_types::PixelEncoding;
use glam::{DVec2, DVec3};
use patching::{Breakdown, Patching};
use shapes::Laid;
use std::collections::{HashMap, HashSet};

pub(crate) const SYSTEM_CONFIG_FILE: &str = "SystemData/SystemConfig.xml";
pub(crate) const MODULE_STORE_FILE: &str = "SystemData/ModuleStore.xml";

const COLOR_BREAKDOWN: &str = "ab38a16f-0de1-4f6e-a8c0-ae5b20d347e0";

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

/// A shape's own name, else its type; the shape's fixture takes the name
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

/// Each shape split into the fixtures its element groups need: a group
/// whose imported elements all lie in one shape, without being all of it,
/// is a part
/// of that shape, and the shape's pixels that lie in the same set of parts
/// become one fixture. Each part is then a union of whole fixtures, so it
/// can be a Donder group: a mega tree splits into its strings, concentric
/// stars into each ring's points.
fn split<'a>(
    placed: Vec<Placed<'a>>,
    config: &'a Config,
    leaves: &HashMap<&'a str, HashSet<&'a str>>,
    exact: &HashMap<Vec<&'a str>, &'a str>,
    taken: &mut HashSet<String>,
) -> Vec<Placed<'a>> {
    let shape_of: HashMap<&str, usize> = placed
        .iter()
        .enumerate()
        .flat_map(|(index, shape)| shape.nodes.iter().map(move |node| (*node, index)))
        .collect();
    // Each shape's parts, by group node, in tree order.
    let mut parts: Vec<Vec<&HashSet<&str>>> = vec![Vec::new(); placed.len()];
    for group in &config.node_order {
        let Some(group_leaves) = leaves.get(group.as_str()) else {
            continue;
        };
        let mut shapes = group_leaves.iter().map(|leaf| shape_of[leaf]);
        let Some(shape) = shapes.next() else {
            continue;
        };
        if shapes.all(|other| other == shape) && group_leaves.len() < placed[shape].nodes.len() {
            parts[shape].push(group_leaves);
        }
    }
    // Each fixture's pixels, and the group of exactly its elements, if any.
    let mut cells: Vec<(Placed, Option<&str>)> = Vec::new();
    for (shape, shape_parts) in placed.into_iter().zip(parts) {
        let part_pixels: Vec<HashSet<usize>> = shape_parts
            .iter()
            .map(|part| {
                (0..shape.nodes.len())
                    .filter(|&pixel| part.contains(shape.nodes[pixel]))
                    .collect()
            })
            .collect();
        let sets = crate::setup::cells(shape.nodes.len(), &part_pixels);
        let count = sets.len();
        for (index, pixels) in sets.iter().enumerate() {
            let nodes: Vec<&str> = pixels.iter().map(|&pixel| shape.nodes[pixel]).collect();
            let mut key = nodes.clone();
            key.sort_unstable();
            let group = exact
                .get(&key)
                .and_then(|group| config.nodes.get(*group))
                .map(|group| group.name.as_str());
            let name = if count == 1 {
                shape.name.clone()
            } else {
                format!("{} {}", shape.name, index + 1)
            };
            let laid = Laid {
                pixels: pixels
                    .iter()
                    .map(|&pixel| shape.laid.pixels[pixel].clone())
                    .collect(),
                ellipses: shape.laid.ellipses.clone(),
            };
            cells.push((Placed { name, laid, nodes }, group));
        }
    }
    // A fixture takes the name of the group of exactly its elements, else
    // its shape's; group names are claimed first, so a shape's name never
    // takes a group's.
    let mut names: Vec<Option<String>> = cells
        .iter()
        .map(|(_, group)| group.map(|group| unique(group.to_string(), taken)))
        .collect();
    for ((cell, _), name) in cells.iter().zip(&mut names) {
        if name.is_none() {
            *name = Some(unique(cell.name.clone(), taken));
        }
    }
    cells
        .into_iter()
        .zip(names)
        .map(|((cell, _), name)| Placed {
            name: name.unwrap_or(cell.name.clone()),
            ..cell
        })
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
    let mut wanted: HashSet<String> = config
        .filters
        .iter()
        .filter(|(_, kind)| kind.as_str() == COLOR_BREAKDOWN)
        .map(|(instance, _)| instance.clone())
        .collect();
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
    let breakdowns: HashMap<String, Result<Breakdown, contract::FieldError>> = config
        .filters
        .iter()
        .filter(|(_, kind)| kind.as_str() == COLOR_BREAKDOWN)
        .filter_map(|(instance, _)| {
            Some((instance.clone(), Breakdown::read(modules.get(instance)?)))
        })
        .collect();
    let patching = Patching::new(&config, &breakdowns, &controllers.slots);

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
    let mut lit: HashMap<&str, usize> = HashMap::new();
    for shape in shapes_list {
        let laid = shapes::layout(shape);
        let laid = match laid {
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
            .find(|node| lit.contains_key(*node) || !seen.insert(**node));
        if let Some(node) = twice {
            report.skipped.push(Skipped {
                model: name,
                reason: SkipReason::LitTwice(config.nodes[*node].name.clone()),
            });
            continue;
        }
        for node in &nodes {
            lit.insert(node, placed.len());
        }
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
            let set: HashSet<&str> = set
                .into_iter()
                .filter(|leaf| lit.contains_key(leaf))
                .collect();
            (group, set)
        })
        .filter(|(_, set)| !set.is_empty())
        .collect();
    let exact = exact_groups(&config, &leaves);
    let placed = split(placed, &config, &leaves, &exact, &mut taken);

    // Each pixel's channels, in the shape's order.
    let patched: Vec<Vec<Result<(Slot, PixelEncoding), crate::report::ElementPatch>>> = placed
        .iter()
        .map(|shape| {
            shape
                .nodes
                .iter()
                .map(|node| {
                    let channel = config.nodes[*node].channel.as_deref().unwrap_or_default();
                    patching.pixel(channel)
                })
                .collect()
        })
        .collect();

    let spacing = options.pixel_spacing.unwrap_or(TYPICAL_PIXEL_SPACING);
    let gaps = placed.iter().flat_map(|shape| {
        shape
            .laid
            .pixels
            .windows(2)
            .map(|pair| pair[0].position.distance(pair[1].position))
            .collect::<Vec<_>>()
    });
    let meters = crate::setup::typical_scale(gaps, spacing);
    report.meters_per_unit = meters;
    report.scale = Some(Scale::PixelSpacing(spacing));
    // Preview units run down the screen; layout y runs up.
    let place = |point: DVec2| DVec3::new(point.x, -point.y, 0.0) * meters;
    let vector = |vector: DVec2| DVec3::new(vector.x, -vector.y, 0.0) * meters;

    let mut fixtures = Vec::new();
    for (shape, pixel_patches) in placed.iter().zip(patched) {
        let mut order: Vec<usize> = (0..shape.laid.pixels.len()).collect();
        let slots = match pixel_patches
            .iter()
            .zip(&shape.nodes)
            .map(|(patch, node)| {
                patch.clone().map_err(|problem| UnroutedReason::Element {
                    element: config.nodes[*node].name.clone(),
                    problem,
                })
            })
            .collect::<Result<Vec<(Slot, PixelEncoding)>, _>>()
        {
            Ok(patches) if patches.windows(2).all(|pair| pair[0].1 == pair[1].1) => {
                // A fixture's pixels go in channel order, so its routes are
                // runs of consecutive channels.
                order.sort_by_key(|&index| {
                    let slot = patches[index].0;
                    (slot.controller, slot.output, slot.offset)
                });
                Ok((
                    order
                        .iter()
                        .map(|&index| patches[index].0)
                        .collect::<Vec<_>>(),
                    patches[0].1,
                ))
            }
            Ok(_) => Err(UnroutedReason::MixedEncodings),
            Err(reason) => Err(reason),
        };
        let (slots, encoding) = match slots {
            Ok((slots, encoding)) => (Some(slots), encoding),
            Err(reason) => {
                report.unrouted.push(Unrouted {
                    model: shape.name.clone(),
                    reason,
                });
                // An unpatched fixture has no routes for its order to apply to.
                (None, PixelEncoding::Rgb { order: [0, 1, 2] })
            }
        };
        fixtures.push(ProfileFixture {
            name: shape.name.clone(),
            description: None,
            pixels: order
                .iter()
                .map(|&index| {
                    let pixel = &shape.laid.pixels[index];
                    Pixel {
                        position: place(pixel.position),
                        arc: pixel.arc,
                    }
                })
                .collect(),
            ellipses: shape
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
            dimming: None,
            slots,
        });
    }

    let groups = groups(&config, &leaves, &placed, &mut taken, &mut report);
    Ok(Profile {
        fixtures,
        controllers: controllers.controllers,
        groups,
        report,
    })
}

/// The element tree's groups that hold whole fixtures, children before the
/// groups holding them. A group holds the groups under it that are
/// imported, and the fixtures of its other elements.
struct GroupWalk<'a> {
    config: &'a Config,
    leaves: &'a HashMap<&'a str, HashSet<&'a str>>,
    placed: &'a [Placed<'a>],
    /// The fixture each element is in.
    fixture_of: HashMap<&'a str, usize>,
    /// Each imported group's name, by node id.
    names: HashMap<&'a str, String>,
    visited: HashSet<&'a str>,
    groups: Vec<ProfileGroup>,
}

impl<'a> GroupWalk<'a> {
    /// The fixtures holding any of `leaves`, in fixture order.
    fn fixtures(&self, leaves: impl IntoIterator<Item = &'a str>) -> Vec<usize> {
        let mut fixtures: Vec<usize> = leaves
            .into_iter()
            .filter_map(|leaf| self.fixture_of.get(leaf).copied())
            .collect();
        fixtures.sort_unstable();
        fixtures.dedup();
        fixtures
    }

    fn child_leaves(&self, child: &'a str) -> Vec<&'a str> {
        match self.leaves.get(child) {
            Some(leaves) => leaves.iter().copied().collect(),
            None => vec![child],
        }
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
        let Some(group_leaves) = self.leaves.get(node) else {
            report.skipped_groups.push(SkippedGroup {
                group: entry.name.clone(),
                reason: GroupSkip::Empty,
            });
            return;
        };
        let held = self.fixtures(group_leaves.iter().copied());
        let skip = |reason| SkippedGroup {
            group: entry.name.clone(),
            reason,
        };
        if held.is_empty() {
            report.skipped_groups.push(skip(GroupSkip::Empty));
            return;
        }
        if let Some(&part) = held.iter().find(|fixture| {
            self.placed[**fixture]
                .nodes
                .iter()
                .any(|node| !group_leaves.contains(node))
        }) {
            let fixture = self.placed[part].name.clone();
            report
                .skipped_groups
                .push(skip(GroupSkip::PartFixture(fixture)));
            return;
        }
        // A group of exactly one fixture's elements is another name for
        // that fixture, which already takes the first such group's name.
        if let [only] = held[..]
            && self.placed[only].nodes.len() == group_leaves.len()
        {
            return;
        }
        // Members in the order of the group's children: an imported child
        // group, or the fixtures of a child's elements that no imported
        // child group holds.
        let covered: HashSet<usize> = entry
            .children
            .iter()
            .filter(|child| self.names.contains_key(child.as_str()))
            .flat_map(|child| self.fixtures(self.child_leaves(child)))
            .collect();
        let mut members = Vec::new();
        let mut listed: HashSet<usize> = HashSet::new();
        for child in &entry.children {
            if let Some(name) = self.names.get(child.as_str()) {
                members.push(name.clone());
                continue;
            }
            for fixture in self.fixtures(self.child_leaves(child)) {
                if !covered.contains(&fixture) && listed.insert(fixture) {
                    members.push(self.placed[fixture].name.clone());
                }
            }
        }
        let name = unique(entry.name.clone(), taken);
        self.names.insert(node, name.clone());
        self.groups.push(ProfileGroup { name, members });
    }
}

fn groups<'a>(
    config: &'a Config,
    leaves: &'a HashMap<&'a str, HashSet<&'a str>>,
    placed: &'a [Placed<'a>],
    taken: &mut HashSet<String>,
    report: &mut ImportReport,
) -> Vec<ProfileGroup> {
    let mut walk = GroupWalk {
        config,
        leaves,
        placed,
        fixture_of: placed
            .iter()
            .enumerate()
            .flat_map(|(index, shape)| shape.nodes.iter().map(move |node| (*node, index)))
            .collect(),
        names: HashMap::new(),
        visited: HashSet::new(),
        groups: Vec::new(),
    };
    for root in &config.roots {
        walk.visit(root, taken, report);
    }
    walk.groups
}
