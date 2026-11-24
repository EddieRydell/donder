//! What a person reads about the repairs an edit makes to objects it left
//! alone, to confirm them before the edit is applied.
use donder_model::{Console, Layout, Patch, Repair, Sequence};
use donder_project_io::ProjectSession;

/// The changes among `repairs` that need confirmation, one sentence for each
/// kind of change in each object, named as `before` held them. Empty when the
/// edit can be applied without asking.
pub fn repair_changes(before: &ProjectSession, repairs: &[Repair]) -> Vec<String> {
    let mut groups: Vec<Group> = Vec::new();
    let described = repairs
        .iter()
        .filter(|repair| repair.needs_confirmation())
        .flat_map(|repair| match repair {
            // Each fixture counts on its own.
            Repair::RemoveLayoutItems { layout, items } => items
                .iter()
                .map(|item| {
                    let (change, object, _) = describe(before, repair);
                    let name = before
                        .project()
                        .get::<Layout>(*layout)
                        .and_then(|layout| layout.fixture(*item))
                        .map(|fixture| fixture.name.as_str().to_string());
                    (change, object, name)
                })
                .collect(),
            _ => vec![describe(before, repair)],
        });
    for (change, object, member) in described {
        match groups
            .iter_mut()
            .find(|group| group.change == change && group.object == object)
        {
            Some(group) => {
                group.count += 1;
                group.members.extend(member);
            }
            None => groups.push(Group {
                change,
                object,
                members: member.into_iter().collect(),
                count: 1,
            }),
        }
    }
    groups.iter().map(Group::sentence).collect()
}

/// One kind of change in one object: how many members, and the names of
/// those that have one.
struct Group {
    change: Change,
    object: String,
    members: Vec<String>,
    count: usize,
}

impl Group {
    fn sentence(&self) -> String {
        let count = self.count;
        let (verb, noun) = self.change.words();
        let noun = if count == 1 {
            noun.to_string()
        } else {
            format!("{noun}s")
        };
        let mut sentence = format!("{verb} {count} {noun} in {}", self.object);
        if !self.members.is_empty() {
            sentence.push_str(": ");
            sentence.push_str(&names(&self.members));
        }
        sentence
    }
}

/// `a`, `a and b`, `a, b and c`, or the first three and how many more.
fn names(members: &[String]) -> String {
    const SHOWN: usize = 3;
    match members {
        [] => String::new(),
        [only] => only.clone(),
        _ if members.len() > SHOWN => format!(
            "{} and {} more",
            members[..SHOWN].join(", "),
            members.len() - SHOWN
        ),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Change {
    RemoveFixture,
    RemoveOutput,
    ShortenOutput,
    RemoveClip,
    TrimClip,
    TrimMarks,
    RemoveEnvelope,
    TrimEnvelope,
    UnbindEnvelope,
    ResetParam,
    RemoveEvent,
    RemoveSet,
    RemovePlayed,
}

impl Change {
    fn words(self) -> (&'static str, &'static str) {
        match self {
            Self::RemoveFixture => ("Removes", "fixture"),
            Self::RemoveOutput => ("Removes", "output"),
            Self::ShortenOutput => ("Shortens", "output"),
            Self::RemoveClip => ("Removes", "clip"),
            Self::TrimClip => ("Trims", "clip"),
            Self::TrimMarks => ("Removes marks past the end from", "mark collection"),
            Self::RemoveEnvelope => ("Removes", "envelope"),
            Self::TrimEnvelope => ("Trims", "envelope"),
            Self::UnbindEnvelope => ("Unbinds", "envelope"),
            Self::ResetParam => ("Resets", "parameter"),
            Self::RemoveEvent => ("Removes", "control event"),
            Self::RemoveSet => ("Removes", "parameter control"),
            Self::RemovePlayed => ("Removes", "played value"),
        }
    }
}

/// A repair's change, the object it is in, and the member's name when it has
/// one.
fn describe(before: &ProjectSession, repair: &Repair) -> (Change, String, Option<String>) {
    let project = before.project();
    let address = |id: donder_model::ObjectId| before.address_of(id).to_string();
    let output = |patch: donder_model::PatchId, route: donder_model::PixelRouteId| {
        let route = project
            .get::<Patch>(patch)?
            .routes
            .iter()
            .find(|candidate| candidate.id == route)?;
        let fixture = project
            .get::<Layout>(route.target.layout)?
            .fixture(route.target.fixture)?;
        Some(fixture.name.as_str().to_string())
    };
    let clip = |sequence: donder_model::SequenceId, clip: &donder_model::ClipId| {
        project
            .get::<Sequence>(sequence)?
            .clips
            .iter()
            .find(|candidate| &candidate.id == clip)
            .map(|clip| clip.name.as_str().to_string())
    };
    let console_clip = |console: donder_model::ConsoleId, clip: donder_model::ConsoleClipId| {
        project
            .get::<Console>(console)?
            .clip(clip)
            .map(|clip| clip.name.as_str().to_string())
    };
    match repair {
        Repair::RemoveLayoutItems { layout, .. } => {
            (Change::RemoveFixture, address((*layout).into()), None)
        }
        Repair::RemoveRoute { patch, route } => (
            Change::RemoveOutput,
            address((*patch).into()),
            output(*patch, *route),
        ),
        Repair::ShortenRoute { patch, route, .. } => (
            Change::ShortenOutput,
            address((*patch).into()),
            output(*patch, *route),
        ),
        Repair::RemoveClip { sequence, clip: id } => (
            Change::RemoveClip,
            address((*sequence).into()),
            clip(*sequence, id),
        ),
        Repair::TrimClip {
            sequence, clip: id, ..
        } => (
            Change::TrimClip,
            address((*sequence).into()),
            clip(*sequence, id),
        ),
        Repair::TrimMarks {
            sequence,
            collection,
        } => (
            Change::TrimMarks,
            address((*sequence).into()),
            Some(collection.as_str().to_string()),
        ),
        Repair::RemoveAutomationClip { sequence, .. } => {
            (Change::RemoveEnvelope, address((*sequence).into()), None)
        }
        Repair::TrimAutomationClip { sequence, .. } => {
            (Change::TrimEnvelope, address((*sequence).into()), None)
        }
        Repair::Unbind { sequence, .. } => {
            (Change::UnbindEnvelope, address((*sequence).into()), None)
        }
        Repair::ResetParam { owner, param } => {
            let (object, holder) = match owner {
                donder_model::ParamOwner::Clip { sequence, clip: id } => {
                    (address((*sequence).into()), clip(*sequence, id))
                }
                donder_model::ParamOwner::ConsoleClip { console, clip } => {
                    (address((*console).into()), console_clip(*console, *clip))
                }
                donder_model::ParamOwner::Node { graph, .. } => (
                    match graph {
                        donder_model::GraphOwner::Sequence(sequence) => address((*sequence).into()),
                        donder_model::GraphOwner::Console(console) => address((*console).into()),
                    },
                    None,
                ),
            };
            let name = match holder {
                Some(holder) => format!("{} of {holder}", param.as_str()),
                None => param.as_str().to_string(),
            };
            (Change::ResetParam, object, Some(name))
        }
        Repair::RemoveConsoleClip { console, clip } => (
            Change::RemoveClip,
            address((*console).into()),
            console_clip(*console, *clip),
        ),
        Repair::RemoveEvent { console, .. } => {
            (Change::RemoveEvent, address((*console).into()), None)
        }
        Repair::RemoveSet { console, param, .. } => (
            Change::RemoveSet,
            address((*console).into()),
            Some(param.as_str().to_string()),
        ),
        Repair::DropPlayed {
            console,
            clip,
            param,
        }
        | Repair::ForgetPlayed {
            console,
            clip,
            param,
        } => (
            Change::RemovePlayed,
            address((*console).into()),
            Some(match console_clip(*console, *clip) {
                Some(clip) => format!("{} of {clip}", param.as_str()),
                None => param.as_str().to_string(),
            }),
        ),
        Repair::DetachBindings { sequence, .. } => {
            (Change::UnbindEnvelope, address((*sequence).into()), None)
        }
        Repair::ForgetAlways { console } | Repair::ForgetActions { console, .. } => {
            (Change::RemoveEvent, address((*console).into()), None)
        }
    }
}
