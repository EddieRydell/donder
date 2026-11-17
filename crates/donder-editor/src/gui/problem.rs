//! Where a refused edit shows: the field of the open editor a broken rule or
//! a refused value concerns, from the model's problem site and the edit.
use donder_model::{
    ClipProblem, ConsoleValidationError, ControllerValidationError, EventProblem,
    InputDeviceValidationError, LayoutError, Member, MemberPart, ProjectValidationError,
    RegionProblem, RouteProblem, SequenceValidationError, SetupProblem, TimingProblem,
    ValidationProblem,
};
use donder_project_io::ProjectSession;

use super::{GuiMutationError, apply_edit, resolve_request};
use crate::dto::{
    CompositionGuiEdit, ConsoleClipEdit, ConsoleGuiEdit, FixtureGuiEdit, GuiDocumentRequest,
    GuiEditCommand, GuiField, GuiItem, GuiMember, GuiOwnershipEdit, GuiProblem, GuiProblemSite,
    LayoutGuiEdit, SequenceAutomationTarget, SequenceClipCommonEdit, SequenceClipRef,
    SequenceGuiEdit,
};

/// What a person reads about `error`, raised by an edit of the object
/// `request` opens in `before`, and the field it concerns. `edited` is the
/// session the edit was made in; `edit_site` is the field the edit set.
pub fn refusal(
    before: &ProjectSession,
    edited: &ProjectSession,
    request: &GuiDocumentRequest,
    edit_site: Option<GuiProblemSite>,
    error: GuiMutationError,
) -> GuiProblem {
    match error {
        GuiMutationError::Invalid(message) => problem(message, edit_site),
        GuiMutationError::At(site, message) => problem(message, Some(site)),
        GuiMutationError::Rejected(rejection) => problem(rejection.to_string(), edit_site),
        GuiMutationError::Project(rule) => match concerned_site(before, request, &rule) {
            Some(site) => problem(rule.problem.to_string(), Some(merged(site, edit_site))),
            None => problem(
                donder_project_io::validation_message(edited, &rule),
                edit_site,
            ),
        },
    }
}

/// The problem `edit` of the object `request` opens would raise; none when
/// it would be accepted. The session does not change.
pub fn check_edit(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
    edit: GuiEditCommand,
) -> Option<GuiProblem> {
    let site = edit_site(&edit);
    let mut draft = session.clone();
    let error = apply_edit(&mut draft, request, edit).err()?;
    Some(refusal(session, &draft, request, site, error))
}

/// A problem whose message starts as a sentence.
pub(crate) fn problem(message: String, site: Option<GuiProblemSite>) -> GuiProblem {
    let mut chars = message.chars();
    let message = chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    GuiProblem { message, site }
}

/// The site of a broken rule in the open editor: the rule concerns the open
/// object or one the edit created, and names a member or field of it.
fn concerned_site(
    before: &ProjectSession,
    request: &GuiDocumentRequest,
    rule: &ProjectValidationError,
) -> Option<GuiProblemSite> {
    let resolved = resolve_request(before, request).ok()?;
    // The patch's outputs break setup rules when they leave the setup's layout or controllers.
    let setup_route = matches!(
        rule.problem,
        ValidationProblem::Setup(
            SetupProblem::RouteOnOtherLayout { .. } | SetupProblem::RouteControllerInactive { .. }
        )
    ) && resolved.kind == donder_model::ObjectKind::Patch;
    let concerns =
        rule.object == resolved.object || before.address_of(rule.object).is_none() || setup_route;
    concerns.then(|| site_of(&rule.problem)).flatten()
}

/// The model's site, with the field of the edit when the model names only
/// the member the edit set.
fn merged(site: GuiProblemSite, edit_site: Option<GuiProblemSite>) -> GuiProblemSite {
    match edit_site {
        Some(edited) if site.field.is_none() && edited.member == site.member => edited,
        _ => site,
    }
}

fn site_of(problem: &ValidationProblem) -> Option<GuiProblemSite> {
    match problem {
        ValidationProblem::Setup(SetupProblem::RouteOnOtherLayout { route, .. }) => {
            return Some(GuiProblemSite::of(
                GuiMember::Route { id: route.0 },
                Some(GuiField::Layout),
            ));
        }
        ValidationProblem::Setup(SetupProblem::RouteControllerInactive { route, .. }) => {
            return Some(GuiProblemSite::of(
                GuiMember::Route { id: route.0 },
                Some(GuiField::Controller),
            ));
        }
        _ => {}
    }
    let field = field_of(problem);
    match problem.site() {
        Some(site) => Some(GuiProblemSite::of(
            member(&site.member),
            site.part.map(part).or(field),
        )),
        None => field.map(GuiProblemSite::object),
    }
}

fn member(member: &Member) -> GuiMember {
    match member {
        Member::Layer(id) => GuiMember::Layer { id: id.0 },
        Member::Clip(id) => GuiMember::Clip { id: id.0 },
        Member::ConsoleClip(id) => GuiMember::Clip { id: id.0 },
        Member::Marks(name) | Member::LiveMarks(name) => GuiMember::MarkCollection {
            key: name.as_str().to_string(),
        },
        Member::AutomationClip(id) => GuiMember::AutomationClip { id: id.0 },
        Member::Node(id) => GuiMember::GraphNode { node_id: id.0 },
        Member::Event(id) => GuiMember::Event { id: id.0 },
        Member::Control(id) => GuiMember::Control { id: id.0 },
        Member::Port(id) => GuiMember::Port { id: id.0 },
        Member::LayoutItem(id) => GuiMember::LayoutItem { id: id.0 },
        Member::Route(id) => GuiMember::Route { id: id.0 },
        Member::Shape(id) => GuiMember::Shape { id: id.0 },
    }
}

fn part(part: MemberPart) -> GuiField {
    match part {
        MemberPart::Param(name) => GuiField::Param {
            name: name.as_str().to_string(),
        },
        MemberPart::PlayedValue(name) => GuiField::PlayedValue {
            name: name.as_str().to_string(),
        },
        MemberPart::Input => GuiField::Input,
        MemberPart::Shape(_) => GuiField::Definition,
    }
}

/// The field a problem's kind concerns, beyond the member the model names.
fn field_of(problem: &ValidationProblem) -> Option<GuiField> {
    match problem {
        ValidationProblem::Route { problem, .. } => route_field(problem),
        ValidationProblem::Controller(error) => controller_field(error),
        ValidationProblem::InputDevice(error) => Some(match error {
            InputDeviceValidationError::DuplicateControl(_) => return None,
            InputDeviceValidationError::InvalidControlName(..)
            | InputDeviceValidationError::DuplicateControlName(..) => GuiField::Name,
            InputDeviceValidationError::DuplicateInput { .. }
            | InputDeviceValidationError::ForeignInput(..)
            | InputDeviceValidationError::InputOutOfRange(..)
            | InputDeviceValidationError::InvalidAddress(..)
            | InputDeviceValidationError::InvalidAxisRange(..) => GuiField::Input,
            InputDeviceValidationError::InvalidOscPort => GuiField::Port,
        }),
        ValidationProblem::Sequence(error) => match error {
            SequenceValidationError::Timing(timing) => Some(timing_field(*timing)),
            SequenceValidationError::Name(_) => Some(GuiField::Name),
            SequenceValidationError::Clip { problem, .. } => clip_field(problem),
            _ => None,
        },
        ValidationProblem::Console(error) => match error {
            ConsoleValidationError::Timing(timing) => Some(timing_field(*timing)),
            ConsoleValidationError::Name(_) => Some(GuiField::Name),
            ConsoleValidationError::Clip { problem, .. } => clip_field(problem),
            ConsoleValidationError::AlwaysLength { .. } => Some(GuiField::Length),
            ConsoleValidationError::Event { problem, .. } => match problem {
                EventProblem::Copies(_) => Some(GuiField::Copies),
                EventProblem::DurationBeyondClock
                | EventProblem::NonPositiveDuration
                | EventProblem::ShortestAboveLongest => Some(GuiField::Length),
                EventProblem::MissingClip => Some(GuiField::Effect),
                _ => None,
            },
            _ => None,
        },
        ValidationProblem::Layout(error) => match error {
            LayoutError::InvalidName(..) | LayoutError::DuplicateName(..) => Some(GuiField::Name),
            LayoutError::InvalidTransform(_) => Some(GuiField::Placement),
            LayoutError::MissingDefinition(_) => Some(GuiField::Definition),
            _ => None,
        },
        ValidationProblem::Curve(_) | ValidationProblem::Gradient(_) => Some(GuiField::Points),
        _ => None,
    }
}

fn route_field(problem: &RouteProblem) -> Option<GuiField> {
    Some(match problem {
        RouteProblem::DuplicateId => return None,
        RouteProblem::InvalidColorOrder => GuiField::ColorOrder,
        RouteProblem::InvalidGamma => GuiField::Gamma,
        RouteProblem::InvalidBrightness => GuiField::Brightness,
        RouteProblem::MissingLayout => GuiField::Layout,
        RouteProblem::TargetNotFixture | RouteProblem::Target(_) => GuiField::Fixture,
        RouteProblem::EmptySpan | RouteProblem::SpanExceedsTarget { .. } => GuiField::Pixels,
        RouteProblem::MissingController => GuiField::Controller,
        RouteProblem::PortOnUniverses | RouteProblem::NeedsPort | RouteProblem::MissingPort => {
            GuiField::Port
        }
        RouteProblem::ChannelOverflow
        | RouteProblem::PastLastChannel { .. }
        | RouteProblem::OutputsOverlap { .. } => GuiField::FirstChannel,
    })
}

fn controller_field(error: &ControllerValidationError) -> Option<GuiField> {
    Some(match error {
        ControllerValidationError::EmptySenderName => GuiField::SenderName,
        ControllerValidationError::InvalidPriority(_) => GuiField::Priority,
        ControllerValidationError::NoUniverses => GuiField::UniverseCount,
        ControllerValidationError::InvalidUniverseChannels(_) => GuiField::UniverseChannels,
        ControllerValidationError::E131UniversesOutOfRange(_)
        | ControllerValidationError::ArtNetUniversesOutOfRange(_) => GuiField::UniverseStart,
        ControllerValidationError::InvalidPortName(_)
        | ControllerValidationError::DuplicatePortName(_) => GuiField::Name,
        ControllerValidationError::EmptyPort(_) => GuiField::Channels,
        ControllerValidationError::DuplicatePort(_)
        | ControllerValidationError::InvalidDonderOutput { .. }
        | ControllerValidationError::DuplicateDonderOutput(_) => return None,
    })
}

fn timing_field(problem: TimingProblem) -> GuiField {
    match problem {
        TimingProblem::ZeroFrameRate | TimingProblem::FrameRateAboveLimit => GuiField::FrameRate,
        TimingProblem::ZeroDuration
        | TimingProblem::DurationBeyondClock
        | TimingProblem::DurationRoundsToZero
        | TimingProblem::TooManyFrames => GuiField::Duration,
    }
}

fn clip_field(problem: &ClipProblem) -> Option<GuiField> {
    Some(match problem {
        ClipProblem::MissingLayer => GuiField::Layer,
        ClipProblem::Region(RegionProblem::StartBeyondClock) => GuiField::Start,
        ClipProblem::Region(_) => GuiField::Duration,
        ClipProblem::MissingDefinition => GuiField::Effect,
        ClipProblem::NotInActiveLayout
        | ClipProblem::MixedLayouts
        | ClipProblem::MissingTargetLayout
        | ClipProblem::MissingTarget => GuiField::Target,
        ClipProblem::Param(_) => return None,
    })
}

/// The member of the open object a generic edit names.
pub(crate) fn item_member(item: &GuiItem) -> GuiMember {
    match item {
        GuiItem::Object => GuiMember::Object,
        GuiItem::Clip { id } => GuiMember::Clip { id: *id },
        GuiItem::MarkCollection { key } => GuiMember::MarkCollection { key: key.clone() },
        GuiItem::Layer { id } => GuiMember::Layer { id: *id },
        GuiItem::GraphNode { node_id } => GuiMember::GraphNode { node_id: *node_id },
        GuiItem::Event { id } => GuiMember::Event { id: *id },
    }
}

/// The field `edit` sets, when it sets one the editor shows.
pub fn edit_site(edit: &GuiEditCommand) -> Option<GuiProblemSite> {
    let on = |member: GuiMember, field: GuiField| Some(GuiProblemSite::of(member, Some(field)));
    match edit {
        GuiEditCommand::Rename { item, .. } => on(item_member(item), GuiField::Name),
        GuiEditCommand::SetDescription { item, .. } => on(item_member(item), GuiField::Description),
        GuiEditCommand::SetColor { item, .. } => on(item_member(item), GuiField::Color),
        GuiEditCommand::Ownership { edit, .. } => Some(GuiProblemSite::object(match edit {
            GuiOwnershipEdit::MakeReusable { .. } => GuiField::Name,
            GuiOwnershipEdit::UseExisting { .. } => GuiField::Declaration,
            GuiOwnershipEdit::MakeIndependent => return None,
        })),
        GuiEditCommand::Curve { .. } | GuiEditCommand::Gradient { .. } => {
            Some(GuiProblemSite::object(GuiField::Points))
        }
        GuiEditCommand::Sequence { edit } => sequence_edit_site(edit),
        GuiEditCommand::Console { edit } => console_edit_site(edit),
        GuiEditCommand::Layout {
            edit: LayoutGuiEdit::MoveFixture { id, .. },
        } => on(GuiMember::LayoutItem { id: *id }, GuiField::Placement),
        GuiEditCommand::Fixture {
            edit:
                FixtureGuiEdit::MoveElement { id, .. }
                | FixtureGuiEdit::MoveHandle { id, .. }
                | FixtureGuiEdit::ConvertToPixels { id },
        } => Some(GuiProblemSite::of(GuiMember::Shape { id: *id }, None)),
        _ => None,
    }
}

fn sequence_edit_site(edit: &SequenceGuiEdit) -> Option<GuiProblemSite> {
    let clip =
        |id: u32, field: GuiField| Some(GuiProblemSite::of(GuiMember::Clip { id }, Some(field)));
    match edit {
        SequenceGuiEdit::SetDuration { .. } => Some(GuiProblemSite::object(GuiField::Duration)),
        SequenceGuiEdit::ChangeEffectDefinition { id, .. } => clip(*id, GuiField::Effect),
        SequenceGuiEdit::RetargetClip { id, .. } => clip(*id, GuiField::Target),
        SequenceGuiEdit::EditClips { clips, edit } => {
            // Several clips edited together share one inspector, which shows the first one's site.
            let id = clips.iter().find_map(|reference| match reference {
                SequenceClipRef::Clip { id } => Some(*id),
                SequenceClipRef::Automation { .. } => None,
            })?;
            clip(id, clip_edit_field(edit))
        }
        SequenceGuiEdit::Composition { edit } => composition_edit_site(edit),
        _ => None,
    }
}

fn clip_edit_field(edit: &SequenceClipCommonEdit) -> GuiField {
    match edit {
        SequenceClipCommonEdit::Layer { .. } => GuiField::Layer,
        SequenceClipCommonEdit::Scope { .. } => GuiField::Scope,
        SequenceClipCommonEdit::Start { .. } => GuiField::Start,
        SequenceClipCommonEdit::Duration { .. } => GuiField::Duration,
        SequenceClipCommonEdit::Param { name, .. } => GuiField::Param { name: name.clone() },
    }
}

fn console_edit_site(edit: &ConsoleGuiEdit) -> Option<GuiProblemSite> {
    let clip =
        |id: u32, field: GuiField| Some(GuiProblemSite::of(GuiMember::Clip { id }, Some(field)));
    match edit {
        ConsoleGuiEdit::SetFrameRate { .. } => Some(GuiProblemSite::object(GuiField::FrameRate)),
        ConsoleGuiEdit::EditClip { id, edit } => clip(
            *id,
            match edit {
                ConsoleClipEdit::Layer { .. } => GuiField::Layer,
                ConsoleClipEdit::Target { .. } => GuiField::Target,
                ConsoleClipEdit::Scope { .. } => GuiField::Scope,
                ConsoleClipEdit::Definition { .. } => GuiField::Effect,
                ConsoleClipEdit::Param { name, .. } => GuiField::Param { name: name.clone() },
            },
        ),
        ConsoleGuiEdit::SetAlways { clip_id, .. } => clip(*clip_id, GuiField::Length),
        ConsoleGuiEdit::SetActions { id, .. } => {
            Some(GuiProblemSite::of(GuiMember::Event { id: *id }, None))
        }
        ConsoleGuiEdit::ControlParam { target, .. } => Some(target_site(target)),
        ConsoleGuiEdit::Composition { edit } => composition_edit_site(edit),
        _ => None,
    }
}

fn composition_edit_site(edit: &CompositionGuiEdit) -> Option<GuiProblemSite> {
    match edit {
        CompositionGuiEdit::UpdateGraphOperatorParam { node_id, name, .. } => {
            Some(GuiProblemSite::of(
                GuiMember::GraphNode { node_id: *node_id },
                Some(GuiField::Param { name: name.clone() }),
            ))
        }
        _ => None,
    }
}

/// The parameter field an automation target names.
fn target_site(target: &SequenceAutomationTarget) -> GuiProblemSite {
    match target {
        SequenceAutomationTarget::ClipParam { clip_id, param } => GuiProblemSite::of(
            GuiMember::Clip { id: *clip_id },
            Some(GuiField::Param {
                name: param.clone(),
            }),
        ),
        SequenceAutomationTarget::CompositionNodeParam { node_id, param } => GuiProblemSite::of(
            GuiMember::GraphNode { node_id: *node_id },
            Some(GuiField::Param {
                name: param.clone(),
            }),
        ),
    }
}
