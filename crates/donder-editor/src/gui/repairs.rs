//! What a person reads about the repairs an edit makes to objects it left
//! alone, to confirm them before the edit is applied.
use donder_model::{
    CompositionGraphNodeKind, DonderProject, Field, GiveWay, MemberRef, Repair, Site,
};
use donder_project_io::ProjectSession;

/// The changes among `repairs` that need confirmation, one sentence for each
/// kind of change in each object, named as `before` held them. Empty when the
/// edit can be applied without asking.
pub fn repair_changes(before: &ProjectSession, repairs: &[Repair]) -> Vec<String> {
    let mut groups: Vec<Group> = Vec::new();
    for repair in repairs.iter().filter(|repair| repair.needs_confirmation()) {
        let site = &repair.site;
        let Some(member) = &site.member else {
            continue;
        };
        let noun = match &site.field {
            Some(Field::Param(_)) => "parameter",
            Some(Field::Played(_)) => "played value",
            Some(Field::Set(_)) => "parameter control",
            Some(Field::Binding(_)) => "envelope binding",
            _ => member.noun(),
        };
        let object = before.address_of(site.object).object().to_string();
        let name = name(before.project(), site);
        match groups.iter_mut().find(|group| {
            group.action == repair.action && group.noun == noun && group.object == object
        }) {
            Some(group) => {
                group.count += 1;
                group.members.extend(name);
            }
            None => groups.push(Group {
                action: repair.action,
                noun,
                object,
                members: name.into_iter().collect(),
                count: 1,
            }),
        }
    }
    groups.iter().map(Group::sentence).collect()
}

/// One kind of change in one object: how many members, and the names of
/// those that have one.
struct Group {
    action: GiveWay,
    noun: &'static str,
    object: String,
    members: Vec<String>,
    count: usize,
}

impl Group {
    fn sentence(&self) -> String {
        let count = self.count;
        let verb = match self.action {
            GiveWay::Remove => "Removes",
            GiveWay::Shrink => "Shortens",
            GiveWay::Reset => "Resets",
            GiveWay::Forget => "Forgets",
        };
        let noun = if count == 1 {
            self.noun.to_string()
        } else {
            format!("{}s", self.noun)
        };
        let mut sentence = format!("{verb} {count} {noun} in {}", self.object);
        if !self.members.is_empty() {
            sentence.push_str(": ");
            sentence.push_str(&names(&self.members));
        }
        sentence
    }
}

/// The name a person knows the repaired part by: a parameter or played
/// value with the clip it belongs to, a route by its fixture.
fn name(project: &DonderProject, site: &Site) -> Option<String> {
    let member = match project.member(site)? {
        MemberRef::Clip(clip) => Some(clip.name.as_str().to_string()),
        MemberRef::Marks(marks) => Some(marks.name.as_str().to_string()),
        MemberRef::LayoutItem(item) => Some(item.name.as_str().to_string()),
        MemberRef::Route(route) => project
            .layout()
            .object()
            .fixture(route.target)
            .map(|fixture| fixture.name.as_str().to_string()),
        MemberRef::Node(node) => match &node.kind {
            CompositionGraphNodeKind::Operator(operator) => {
                Some(operator.name.as_str().to_string())
            }
            _ => None,
        },
        _ => None,
    };
    let part = match &site.field {
        Some(Field::Param(param) | Field::Played(param) | Field::Set(param)) => param,
        _ => return member,
    };
    Some(match member {
        Some(member) if !matches!(site.field, Some(Field::Set(_))) => {
            format!("{} of {member}", part.as_str())
        }
        _ => part.as_str().to_string(),
    })
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
