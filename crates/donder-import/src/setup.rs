//! A profile read from any source, as the layout, controllers and patch of a
//! Donder project.

use crate::fit::{self, Ellipse, Pixel};
use crate::report::{
    ControllerSkip, DropReason, DroppedMember, ImportReport, SkipReason, Skipped,
    SkippedController, Unrouted, UnroutedReason,
};
use camino::Utf8PathBuf;
use donder_model::{
    Controller, ControllerId, ControllerPort, ControllerPortAddress, ControllerPortId,
    ControllerProtocol, FixtureDefinition, FixtureInstanceId, FixtureTarget, FixtureTransform,
    Layout, LayoutFixture, LayoutFixtureKind, LayoutId, ObjectId, Owned, OwnedMember,
    OwnershipSite, Patch, PatchId, PixelRoute, PixelRouteId, PixelSpan, ProjectEdit, Setup,
};
use donder_project_io::{DocumentId, ProjectSession};
use donder_runtime_types::{Identifier, PixelEncoding};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Debug)]
pub enum ImportError {
    /// The folder holds neither an xLights profile nor a Vixen profile.
    UnknownProfile(Utf8PathBuf),
    /// The folder holds both an xLights profile and a Vixen profile.
    AmbiguousProfile(Utf8PathBuf),
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
    /// The xLights layout's ruler measures in units xLights does not name.
    RulerUnits(i32),
    /// The Vixen profile has no enabled preview.
    NoPreview,
    /// The Vixen profile has several enabled previews, by name.
    SeveralPreviews(Vec<String>),
    /// The Vixen profile has no preview of this name.
    UnknownPreview(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProfile(path) => write!(
                f,
                "{path} holds neither an xLights show folder ({}) nor a Vixen profile ({}).",
                crate::xlights::LAYOUT_FILE,
                crate::vixen::SYSTEM_CONFIG_FILE
            ),
            Self::NoPreview => write!(
                f,
                "The Vixen profile has no enabled preview; name one with --preview."
            ),
            Self::SeveralPreviews(names) => write!(
                f,
                "The Vixen profile has several enabled previews ({}); name one with --preview.",
                names.join(", ")
            ),
            Self::UnknownPreview(name) => {
                write!(f, "The Vixen profile has no preview named {name}.")
            }
            Self::AmbiguousProfile(path) => write!(
                f,
                "{path} holds both an xLights profile and a Vixen profile. choose the folder of one."
            ),
            Self::Read { path, message } => write!(f, "Could not read {path}: {message}"),
            Self::Xml { path, message } => write!(f, "{path} is not readable XML: {message}"),
            Self::NotEmpty => write!(
                f,
                "This project's setup already has fixtures or controllers; import a profile into a new project."
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
                write!(f, "The imported profile is not a valid project: {message}")
            }
        }
    }
}

/// How a fixture's output is dimmed, when it is exactly a brightness and a
/// gamma.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dimming {
    /// Output scale: 1 leaves it as it is.
    pub(crate) brightness: f32,
    pub(crate) gamma: f32,
}

/// Where a pixel's first channel is: a controller of the profile, one of its
/// outputs, and the slot in it from 0.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Slot {
    pub(crate) controller: usize,
    pub(crate) output: usize,
    pub(crate) offset: u32,
}

/// A fixture of the profile: its pixels in channel order.
pub(crate) struct ProfileFixture {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    /// In meters, in layout coordinates.
    pub(crate) pixels: Vec<Pixel>,
    /// In meters, the ellipses the pixels' arcs are on.
    pub(crate) ellipses: Vec<Ellipse>,
    pub(crate) encoding: PixelEncoding,
    pub(crate) dimming: Option<Dimming>,
    /// Each pixel's first slot, or none when the fixture is not patched and
    /// the reader has reported why.
    pub(crate) slots: Option<Vec<Slot>>,
}

impl ProfileFixture {
    /// The fixture of the pixels at `pixels`, in their order here.
    pub(crate) fn subset(&self, name: String, pixels: &[usize]) -> Self {
        Self {
            name,
            description: self.description.clone(),
            pixels: pixels
                .iter()
                .map(|&pixel| self.pixels[pixel].clone())
                .collect(),
            ellipses: self.ellipses.clone(),
            encoding: self.encoding,
            dimming: self.dimming,
            slots: self
                .slots
                .as_ref()
                .map(|slots| pixels.iter().map(|&pixel| slots[pixel]).collect()),
        }
    }
}

/// The cells `parts` split `count` pixels into, each part a set of pixel
/// indices: the pixels in the same set of parts are one cell, so each part
/// is a union of whole cells. Cells are in the order of their first pixel,
/// and each holds its pixels in order.
pub(crate) fn cells(count: usize, parts: &[HashSet<usize>]) -> Vec<Vec<usize>> {
    let mut cells: Vec<(Vec<usize>, Vec<usize>)> = Vec::new();
    for pixel in 0..count {
        let key: Vec<usize> = parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.contains(&pixel))
            .map(|(index, _)| index)
            .collect();
        match cells.iter_mut().find(|(cell, _)| *cell == key) {
            Some((_, pixels)) => pixels.push(pixel),
            None => cells.push((key, vec![pixel])),
        }
    }
    cells.into_iter().map(|(_, pixels)| pixels).collect()
}

/// One universe a controller sends.
pub(crate) struct ProfileOutput {
    pub(crate) universe: u16,
    pub(crate) channels: u16,
}

pub(crate) struct ProfileController {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    /// How Donder sends to it, or why it is not created.
    pub(crate) protocol: Result<ControllerProtocol, ControllerSkip>,
    /// What it sends, by the output index slots refer to.
    pub(crate) outputs: Vec<ProfileOutput>,
}

pub(crate) struct ProfileGroup {
    pub(crate) name: String,
    /// Fixtures and other groups, by their names in the profile.
    pub(crate) members: Vec<String>,
}

/// What a reader makes of a profile.
pub(crate) struct Profile {
    pub(crate) fixtures: Vec<ProfileFixture>,
    pub(crate) controllers: Vec<ProfileController>,
    pub(crate) groups: Vec<ProfileGroup>,
    /// What the reader has reported: its scale, and what it left out.
    pub(crate) report: ImportReport,
}

/// Meters per layout unit when the typical gap between neighbouring pixels
/// is `spacing` meters, from the gaps of each fixture's pixels in channel
/// order, in layout units.
pub(crate) fn typical_scale(gaps: impl IntoIterator<Item = f64>, spacing: f64) -> f64 {
    let mut gaps: Vec<f64> = gaps.into_iter().filter(|gap| *gap > 0.0).collect();
    gaps.sort_by(f64::total_cmp);
    let typical = gaps.get(gaps.len() / 2).copied().unwrap_or(1.0);
    spacing / typical
}

/// The Donder controllers, with each profile output's port.
struct Controllers {
    controllers: Vec<(Identifier, Controller)>,
    /// By profile controller and output.
    ports: HashMap<(usize, usize), (ControllerId, ControllerPortId, u32)>,
}

fn controllers(
    profile: &[ProfileController],
    report: &mut ImportReport,
    mut new_id: impl FnMut() -> u32,
) -> Controllers {
    let mut result = Controllers {
        controllers: Vec::new(),
        ports: HashMap::new(),
    };
    let mut taken = HashSet::new();
    for (index, controller) in profile.iter().enumerate() {
        let protocol = match &controller.protocol {
            Ok(protocol) => protocol.clone(),
            Err(reason) => {
                report.skipped_controllers.push(SkippedController {
                    controller: controller.name.clone(),
                    reason: reason.clone(),
                });
                continue;
            }
        };
        let id = ControllerId(new_id());
        let mut ports = Vec::new();
        for (output_index, output) in controller.outputs.iter().enumerate() {
            let address = match protocol {
                ControllerProtocol::ArtNet(_) => ControllerPortAddress::ArtNetPort(output.universe),
                _ => ControllerPortAddress::E131Universe(output.universe),
            };
            let port = ControllerPortId(ports.len() as u32 + 1);
            ports.push(ControllerPort {
                id: port,
                name: donder_language::object_name(&format!("universe_{}", output.universe)),
                address,
                slot_count: output.channels,
            });
            result.ports.insert(
                (index, output_index),
                (id, port, u32::from(output.channels)),
            );
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

/// A fixture's routes as runs of pixels: the slot of the first, its index,
/// and how many; or why some of its pixels have none.
fn runs(
    fixture: &ProfileFixture,
    profile: &[ProfileController],
    controllers: &Controllers,
) -> Result<Vec<(Slot, u32, u32)>, UnroutedReason> {
    let Some(slots) = &fixture.slots else {
        return Ok(Vec::new());
    };
    let per_pixel = fixture.encoding.channel_order().len() as u32;
    let mut runs: Vec<(Slot, u32, u32)> = Vec::new();
    for (pixel, &slot) in slots.iter().enumerate() {
        let Some(&(_, _, channels)) = controllers.ports.get(&(slot.controller, slot.output)) else {
            return Err(UnroutedReason::UnsupportedController(
                profile[slot.controller].name.clone(),
            ));
        };
        if slot.offset + per_pixel > channels {
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

/// Slots from `0` up to `1` of an output, patched to the fixture named `2`.
type Claim = (u32, u32, String);

/// Put the profile in the project's setup, whose layout, patch and controllers
/// must be empty.
pub(crate) fn build(
    session: &mut ProjectSession,
    profile: Profile,
) -> Result<ImportReport, ImportError> {
    let Profile {
        fixtures: profile_fixtures,
        controllers: profile_controllers,
        groups: profile_groups,
        mut report,
    } = profile;
    let controllers = controllers(&profile_controllers, &mut report, || {
        session.project.new_id()
    });

    // Patch each fixture whose channels no earlier fixture has.
    let layout_id = LayoutId(session.project.new_id());
    let patch_id = PatchId(session.project.new_id());
    let mut claimed: HashMap<(usize, usize), Vec<Claim>> = HashMap::new();
    let mut fixtures = Vec::new();
    let mut route_list = Vec::new();
    let mut by_name: HashMap<String, FixtureInstanceId> = HashMap::new();
    let mut taken: HashSet<String> = HashSet::new();
    for fixture in profile_fixtures {
        let runs = match runs(&fixture, &profile_controllers, &controllers) {
            Ok(runs) => runs,
            Err(reason) => {
                report.unrouted.push(Unrouted {
                    model: fixture.name.clone(),
                    reason,
                });
                Vec::new()
            }
        };
        let per_pixel = fixture.encoding.channel_order().len() as u32;
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
                model: fixture.name,
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
                    fixture.name.clone(),
                ));
        }
        let id = FixtureInstanceId(fixtures.len() as u32 + 1);
        let origin = fixture.pixels[0].position;
        let elements = fit::shapes(&fixture.pixels, &fixture.ellipses, origin);
        let name = donder_language::unique_name(
            donder_language::object_name(&fixture.name).as_str(),
            |name| taken.contains(name),
        );
        taken.insert(name.as_str().to_string());
        by_name.insert(fixture.name.clone(), id);
        let pixel_count = fixture.pixels.len() as u32;
        let (brightness, gamma) = fixture
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
                encoding: fixture.encoding,
                gamma,
                brightness,
            });
        }
        report.pixels += fixture.pixels.len();
        fixtures.push(LayoutFixture {
            id,
            name,
            description: fixture.description,
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

    let (groups, root_members) = groups(
        &profile_groups,
        &by_name,
        fixtures.len(),
        &mut taken,
        &mut report,
    );
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
    profile: &[ProfileGroup],
    fixtures: &HashMap<String, FixtureInstanceId>,
    fixture_count: usize,
    taken: &mut HashSet<String>,
    report: &mut ImportReport,
) -> (Vec<LayoutFixture>, Vec<FixtureInstanceId>) {
    let group_index: HashMap<&str, usize> = profile
        .iter()
        .enumerate()
        .map(|(index, group)| (group.name.as_str(), index))
        .collect();
    enum Member {
        Fixture(FixtureInstanceId),
        Group(usize),
    }
    let mut members: Vec<Vec<Member>> = Vec::new();
    for group in profile {
        let mut list = Vec::new();
        for entry in &group.members {
            let member = if let Some(&fixture) = fixtures.get(entry) {
                Member::Fixture(fixture)
            } else if let Some(&index) = group_index.get(entry.as_str()) {
                Member::Group(index)
            } else {
                report.dropped_members.push(DroppedMember {
                    group: group.name.clone(),
                    member: entry.clone(),
                    reason: DropReason::Missing,
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
                    group: profile[group].name.clone(),
                    member: profile[inner].name.clone(),
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
            donder_language::object_name(&profile[group].name).as_str(),
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
/// own document.
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

/// A name no fixture or group of the profile has yet.
pub(crate) fn unique(name: String, taken: &mut HashSet<String>) -> String {
    let mut candidate = name.clone();
    let mut index = 2;
    while !taken.insert(candidate.clone()) {
        candidate = format!("{name} {index}");
        index += 1;
    }
    candidate
}
