//! A profile read from any source, as the layout, controllers and patch of a
//! Donder project.

use crate::fit::{self, Ellipse, Pixel};
use crate::report::{
    ControllerSkip, DropReason, DroppedMember, ImportReport, Scale, SkipReason, Skipped,
    SkippedController, TYPICAL_PIXEL_SPACING, Unrouted, UnroutedReason,
};
use camino::Utf8PathBuf;
use donder_model::{
    ArtNetConfig, ArtNetMode, Controller, ControllerId, ControllerPort, ControllerPortId,
    ControllerProtocol, ControllerValidationError, CurveDefinition, CurveId, E131Config, E131Mode,
    FixtureDefinition, FixtureInstanceId, FixtureTarget, FixtureTransform, GradientDefinition,
    GradientId, GroupMember, Layout, LayoutFixture, LayoutFixtureKind, LayoutId, Object, ObjectId,
    Owned, OwnedMember, OwnershipSite, Patch, PatchId, PixelRoute, PixelRouteId, PixelSpan,
    ProjectEdit, RouteStart, Setup, Universes,
};
use donder_project_io::{DocumentId, ProjectSession};
use donder_runtime_types::{Curve, Gradient, Identifier, PixelEncoding};
use glam::DVec3;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;

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
    /// Its first channel in the profile's FSEQ files, from 1.
    pub(crate) fseq_channel: NonZeroU32,
}

/// How Donder sends to a controller.
pub(crate) enum ProfileProtocol {
    E131 {
        priority: u8,
        mode: E131Mode,
    },
    /// Art-Net unicast to this address.
    ArtNet(IpAddr),
}

/// The UDP port Art-Net sends and listens on.
const ARTNET_PORT: u16 = 6454;

pub(crate) struct ProfileController {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    /// How Donder sends to it, or why it is not created.
    pub(crate) protocol: Result<ProfileProtocol, ControllerSkip>,
    /// What it sends, by the output index slots refer to.
    pub(crate) outputs: Vec<ProfileOutput>,
}

pub(crate) struct ProfileGroup {
    pub(crate) name: String,
    pub(crate) members: Vec<ProfileMember>,
}

/// A group member: a fixture or another group by its name in the profile, or
/// a run of a fixture's pixels in its pixel order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ProfileMember {
    Named(String),
    Slice {
        fixture: String,
        start: u32,
        count: u32,
    },
}

/// What a reader makes of a profile.
pub(crate) struct Profile {
    pub(crate) fixtures: Vec<ProfileFixture>,
    pub(crate) controllers: Vec<ProfileController>,
    pub(crate) groups: Vec<ProfileGroup>,
    /// The profile's named curves, normalized, and gradients, by their
    /// names in the profile.
    pub(crate) curves: Vec<(String, Curve)>,
    pub(crate) gradients: Vec<(String, Gradient)>,
    /// What the reader has reported: its scale, and what it left out.
    pub(crate) report: ImportReport,
}

/// Meters per layout unit, and how it was set: from the gaps between each
/// fixture's neighbouring pixels in channel order, in layout units, so the
/// typical gap is `pixel_spacing` meters, or [`TYPICAL_PIXEL_SPACING`].
pub(crate) fn typical_scale(
    fixtures: impl IntoIterator<Item = Vec<DVec3>>,
    pixel_spacing: Option<f64>,
) -> (f64, Scale) {
    let spacing = pixel_spacing.unwrap_or(TYPICAL_PIXEL_SPACING);
    let mut gaps: Vec<f64> = fixtures
        .into_iter()
        .flat_map(|pixels| {
            pixels
                .windows(2)
                .map(|pair| pair[0].distance(pair[1]))
                .collect::<Vec<_>>()
        })
        .filter(|gap| *gap > 0.0)
        .collect();
    gaps.sort_by(f64::total_cmp);
    let typical = gaps.get(gaps.len() / 2).copied().unwrap_or(1.0);
    (spacing / typical, Scale::PixelSpacing(spacing))
}

/// The Donder controllers, and where each profile output's channels are in
/// them.
struct Controllers {
    controllers: Vec<(Identifier, Controller)>,
    /// By profile controller and output.
    outputs: HashMap<(usize, usize), OutputChannels>,
}

/// Where a profile output's channels are in a Donder controller.
#[derive(Clone, Copy)]
struct OutputChannels {
    controller: ControllerId,
    /// The output's first channel in the controller, from 0.
    first: u32,
    /// The channels from `first` that follow on without a gap: to the end
    /// of the output's run of universes.
    room: u32,
}

/// How Donder sends to a controller's universes.
fn protocol(
    protocol: &ProfileProtocol,
    universes: Universes,
    fseq_channel: NonZeroU32,
) -> ControllerProtocol {
    let unspecified = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    match protocol {
        ProfileProtocol::E131 { priority, mode } => ControllerProtocol::E131(E131Config {
            sender_name: "Donder".to_string(),
            bind_address: unspecified,
            priority: *priority,
            mode: mode.clone(),
            universes,
            fseq_channel: Some(fseq_channel),
            ports: vec![import_port()],
        }),
        ProfileProtocol::ArtNet(destination) => ControllerProtocol::ArtNet(ArtNetConfig {
            bind_address: SocketAddr::new(unspecified, ARTNET_PORT),
            destination: SocketAddr::new(*destination, ARTNET_PORT),
            mode: ArtNetMode::Unicast,
            universes,
            fseq_channel: Some(fseq_channel),
            ports: vec![import_port()],
        }),
    }
}

/// The one port an imported controller has; profiles do not say which
/// physical output carries which channels.
const IMPORT_PORT: ControllerPortId = ControllerPortId(1);

fn import_port() -> ControllerPort {
    ControllerPort {
        id: IMPORT_PORT,
        name: "1".to_string(),
        description: None,
    }
}

/// Why Donder cannot send to a controller the importer built, or an error
/// when the importer built it wrong.
fn refusal(error: ControllerValidationError) -> Result<ControllerSkip, ImportError> {
    use ControllerValidationError as Invalid;
    Ok(match error {
        Invalid::InvalidPriority(priority) => ControllerSkip::Priority(priority.into()),
        Invalid::E131UniversesOutOfRange(universes)
        | Invalid::ArtNetUniversesOutOfRange(universes) => {
            ControllerSkip::Universe(if universes.start == 0 {
                0
            } else {
                i32::from(universes.start) + i32::from(universes.count) - 1
            })
        }
        Invalid::EmptySenderName
        | Invalid::NoUniverses
        | Invalid::InvalidUniverseChannels(_)
        | Invalid::DuplicatePort(_)
        | Invalid::EmptyPortName(_)
        | Invalid::DuplicatePortName(_)
        | Invalid::EmptyPort(_)
        | Invalid::InvalidDonderOutput { .. }
        | Invalid::DuplicateDonderOutput(_) => {
            return Err(ImportError::Project(error.to_string()));
        }
    })
}

/// A profile controller's outputs as runs of consecutive universes, each a
/// Donder controller: the outputs' indices and the universes. An output
/// continues a run when its universe and FSEQ channels follow the last
/// one's, which carries the run's channels per universe, and it carries no
/// more; a shorter last universe sends its unused channels as zero.
fn universe_runs(
    outputs: &[ProfileOutput],
) -> Result<Vec<(Vec<usize>, Universes)>, ControllerSkip> {
    let mut seen = HashSet::new();
    let mut runs: Vec<(Vec<usize>, Universes)> = Vec::new();
    for (index, output) in outputs.iter().enumerate() {
        if output.channels == 0 {
            return Err(ControllerSkip::EmptyOutput);
        }
        if output.channels > 512 {
            return Err(ControllerSkip::LargeOutput(output.channels.into()));
        }
        if !seen.insert(output.universe) {
            return Err(ControllerSkip::RepeatedUniverse(output.universe.into()));
        }
        let continues = runs.last().is_some_and(|(members, universes)| {
            let last = &outputs[members[members.len() - 1]];
            u32::from(last.universe) + 1 == u32::from(output.universe)
                && last.fseq_channel.checked_add(u32::from(last.channels))
                    == Some(output.fseq_channel)
                && last.channels == universes.channels
                && output.channels <= universes.channels
        });
        match runs.last_mut() {
            Some((members, universes)) if continues => {
                members.push(index);
                universes.count += 1;
            }
            _ => runs.push((
                vec![index],
                Universes {
                    start: output.universe,
                    count: 1,
                    channels: output.channels,
                },
            )),
        }
    }
    Ok(runs)
}

fn controllers(
    profile: &[ProfileController],
    report: &mut ImportReport,
    mut new_id: impl FnMut() -> u32,
) -> Result<Controllers, ImportError> {
    let mut result = Controllers {
        controllers: Vec::new(),
        outputs: HashMap::new(),
    };
    let mut taken = HashSet::new();
    for (index, controller) in profile.iter().enumerate() {
        let skip = |reason| SkippedController {
            controller: controller.name.clone(),
            reason,
        };
        let read = match &controller.protocol {
            Ok(read) => read,
            Err(reason) => {
                report.skipped_controllers.push(skip(reason.clone()));
                continue;
            }
        };
        let runs = match universe_runs(&controller.outputs) {
            Ok(runs) if runs.is_empty() => {
                report
                    .skipped_controllers
                    .push(skip(ControllerSkip::NoOutputs));
                continue;
            }
            Ok(runs) => runs,
            Err(reason) => {
                report.skipped_controllers.push(skip(reason));
                continue;
            }
        };
        let mut built = Vec::new();
        let mut refused = None;
        for (members, universes) in runs {
            let candidate = Controller {
                id: ControllerId(new_id()),
                description: controller.description.clone(),
                protocol: protocol(read, universes, controller.outputs[members[0]].fseq_channel),
            };
            if let Err(error) = candidate.validate() {
                refused = Some(refusal(error)?);
                break;
            }
            built.push((candidate, members, universes));
        }
        if let Some(reason) = refused {
            report.skipped_controllers.push(skip(reason));
            continue;
        }
        for (candidate, members, universes) in built {
            let width = u32::from(universes.channels);
            let last = members.len() as u32 - 1;
            let end =
                last * width + u32::from(controller.outputs[members[members.len() - 1]].channels);
            for (position, &output) in (0..).zip(&members) {
                let first = position * width;
                result.outputs.insert(
                    (index, output),
                    OutputChannels {
                        controller: candidate.id,
                        first,
                        room: end - first,
                    },
                );
            }
            let name = donder_language::unique_name(
                donder_language::object_name(&controller.name).as_str(),
                |name| taken.contains(name),
            );
            taken.insert(name.as_str().to_string());
            result.controllers.push((name, candidate));
        }
    }
    Ok(result)
}

/// A pixel run's place: the Donder controller and the run's first channel
/// in it, from 0.
#[derive(Clone, Copy)]
struct Place {
    controller: ControllerId,
    channel: u32,
}

/// A fixture's routes as runs of pixels: the place of the first, its index,
/// and how many; or why some of its pixels have none.
fn runs(
    fixture: &ProfileFixture,
    profile: &[ProfileController],
    controllers: &Controllers,
) -> Result<Vec<(Place, u32, u32)>, UnroutedReason> {
    let Some(slots) = &fixture.slots else {
        return Ok(Vec::new());
    };
    let per_pixel = fixture.encoding.channel_count() as u32;
    let mut runs: Vec<(Place, u32, u32)> = Vec::new();
    for (pixel, &slot) in slots.iter().enumerate() {
        let Some(&channels) = controllers.outputs.get(&(slot.controller, slot.output)) else {
            return Err(UnroutedReason::UnsupportedController(
                profile[slot.controller].name.clone(),
            ));
        };
        if slot.offset + per_pixel > channels.room {
            return Err(UnroutedReason::SplitPixel);
        }
        let place = Place {
            controller: channels.controller,
            channel: channels.first + slot.offset,
        };
        match runs.last_mut() {
            Some((first, start, count))
                if first.controller == place.controller
                    && first.channel + *count * per_pixel == place.channel
                    && *start + *count == pixel as u32 =>
            {
                *count += 1
            }
            _ => runs.push((place, pixel as u32, 1)),
        }
    }
    Ok(runs)
}

/// Channels from `0` up to `1` of a controller, patched to the fixture named `2`.
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
        curves,
        gradients,
        mut report,
    } = profile;
    let controllers = controllers(&profile_controllers, &mut report, || {
        session.project.new_id()
    })?;

    // Patch each fixture whose channels no earlier fixture has.
    let layout_id = LayoutId(session.project.new_id());
    let patch_id = PatchId(session.project.new_id());
    let mut claimed: HashMap<ControllerId, Vec<Claim>> = HashMap::new();
    let mut fixtures = Vec::new();
    let mut route_list = Vec::new();
    // Where each controller's last route ends, for routes that follow on.
    let mut ends: HashMap<ControllerId, u32> = HashMap::new();
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
        let per_pixel = fixture.encoding.channel_count() as u32;
        let overlap = runs.iter().find_map(|(place, _, count)| {
            let (from, to) = (place.channel, place.channel + count * per_pixel);
            claimed
                .get(&place.controller)?
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
        for (place, _, count) in &runs {
            claimed.entry(place.controller).or_default().push((
                place.channel,
                place.channel + count * per_pixel,
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
        for (place, start, count) in runs {
            let previous_end = ends.insert(place.controller, place.channel + count * per_pixel);
            let follows = previous_end.unwrap_or(0) == place.channel;
            route_list.push(PixelRoute {
                id: PixelRouteId(route_list.len() as u32 + 1),
                target: FixtureTarget {
                    layout: layout_id,
                    fixture: id,
                },
                pixels: (count != pixel_count).then_some(PixelSpan { start, count }),
                controller: place.controller,
                port: IMPORT_PORT,
                start: if follows {
                    RouteStart::Next
                } else {
                    RouteStart::Channel(place.channel)
                },
                encoding: fixture.encoding,
                gamma,
                brightness,
                dither: false,
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
    let curves = curves
        .into_iter()
        .map(|(name, curve)| {
            let id = CurveId(session.project.new_id());
            let definition = CurveDefinition {
                description: None,
                curve,
            };
            (name, id.into(), Object::Curve(id, definition))
        })
        .collect();
    report.curves = library(session, LIBRARY_CURVES, curves)?;
    let gradients = gradients
        .into_iter()
        .map(|(name, gradient)| {
            let id = GradientId(session.project.new_id());
            let definition = GradientDefinition {
                description: None,
                gradient,
            };
            (name, id.into(), Object::Gradient(id, definition))
        })
        .collect();
    report.gradients = library(session, LIBRARY_GRADIENTS, gradients)?;
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
        Slice(FixtureInstanceId, PixelSpan),
        Group(usize),
    }
    let mut members: Vec<Vec<Member>> = Vec::new();
    for group in profile {
        let mut list = Vec::new();
        for entry in &group.members {
            let member = match entry {
                ProfileMember::Named(name) => {
                    if let Some(&fixture) = fixtures.get(name) {
                        Some(Member::Fixture(fixture))
                    } else {
                        group_index
                            .get(name.as_str())
                            .map(|&index| Member::Group(index))
                    }
                }
                ProfileMember::Slice {
                    fixture,
                    start,
                    count,
                } => fixtures.get(fixture).map(|&fixture| {
                    Member::Slice(
                        fixture,
                        PixelSpan {
                            start: *start,
                            count: *count,
                        },
                    )
                }),
            };
            let Some(member) = member else {
                report.dropped_members.push(DroppedMember {
                    group: group.name.clone(),
                    member: match entry {
                        ProfileMember::Named(name) => name.clone(),
                        ProfileMember::Slice { fixture, .. } => fixture.clone(),
                    },
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
                    Member::Fixture(_) | Member::Slice(..) => false,
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
                    Member::Fixture(_) | Member::Slice(..) => false,
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
        let list: Vec<GroupMember> = list
            .iter()
            .filter_map(|member| match member {
                Member::Fixture(fixture) => {
                    grouped.insert(*fixture);
                    Some(GroupMember::Item(*fixture))
                }
                Member::Slice(fixture, pixels) => Some(GroupMember::Slice {
                    fixture: *fixture,
                    pixels: *pixels,
                }),
                Member::Group(inner) => ids
                    .get(inner)
                    .inspect(|inner| {
                        nested.insert(**inner);
                    })
                    .map(|inner| GroupMember::Item(*inner)),
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

/// A file of its own for `id`, named after its kind.
fn new_document(session: &mut ProjectSession, id: ObjectId) -> Result<DocumentId, ImportError> {
    let name = session.source.new_object_name(id.kind());
    session
        .source
        .add_object_document(id, name)
        .map_err(ImportError::Project)
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
    let setup_id = *root.setup.id();
    let setup = project
        .get::<Setup>(setup_id)
        .ok_or_else(|| ImportError::Project("The project has no setup.".into()))?
        .clone();
    let layout_id = *setup.layout.id();
    let patch_id = *setup.patch.id();
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
            new_document(session, id)?
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
    new_document(session, layout.id.into())?;
    new_document(session, patch.id.into())?;
    donder_project_io::maintain_ownership_sources(session)
        .map_err(|error| ImportError::Project(error.to_string()))?;
    Ok(())
}

/// Where a profile's library curves and gradients go.
const LIBRARY_CURVES: &str = "curves/library.data.donder";
const LIBRARY_GRADIENTS: &str = "gradients/library.data.donder";

/// Put library objects in the project, in one document at `path` that the
/// project root imports, each under its object name, numbered on a clash.
/// Returns how many it put.
fn library(
    session: &mut ProjectSession,
    path: &str,
    objects: Vec<(String, ObjectId, Object)>,
) -> Result<usize, ImportError> {
    if objects.is_empty() {
        return Ok(0);
    }
    let invalid = |error: donder_model::EditError| ImportError::Project(format!("{error:?}"));
    let mut taken = HashSet::new();
    let mut names = Vec::new();
    let mut edits = Vec::new();
    for (name, id, object) in objects {
        let name =
            donder_language::unique_name(donder_language::object_name(&name).as_str(), |name| {
                taken.contains(name)
            });
        taken.insert(name.as_str().to_string());
        names.push((id, name));
        edits.push(ProjectEdit::Put(object));
    }
    let count = names.len();
    let first = names[0].0;
    session.project.apply_edits(edits).map_err(invalid)?;
    session
        .source
        .add_data_document(path.into(), names)
        .map_err(ImportError::Project)?;
    let root = session
        .source
        .entrypoint
        .clone()
        .ok_or_else(|| ImportError::Project("The project has no root document.".into()))?;
    donder_project_io::ensure_document_can_reference_object(session, &root, first)
        .map_err(|error| ImportError::Project(error.to_string()))?;
    Ok(count)
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
