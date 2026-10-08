//! Layout fixtures are effect targets. Parts inside a definition are not layout targets.
//!
//! Each fixture is placed once. Groups are ordered member lists that may share
//! members, so membership forms an acyclic graph rooted at `Layout::root`.

mod editing;

use std::collections::HashSet;

use crate::fixture::{FixtureDefinitionId, FixtureGeometryError, FixtureSource, FixtureTransform};
use crate::identity::ObjectIdentity;
use indexmap::IndexMap;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LayoutId(pub ObjectIdentity);

/// Stable across renames, reordering and membership changes; unique in a layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct FixtureInstanceId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub id: LayoutId,
    pub description: Option<String>,
    /// Every fixture and group exactly once. Fixture order is placement order.
    pub fixtures: Vec<LayoutFixture>,
    /// Top-level members in display order.
    pub root: Vec<FixtureInstanceId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutFixture {
    pub id: FixtureInstanceId,
    /// Unique within the layout; references use it.
    pub name: crate::dsl::Identifier,
    pub description: Option<String>,
    pub kind: LayoutFixtureKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LayoutFixtureKind {
    Fixture {
        definition: FixtureSource,
        transform: FixtureTransform,
    },
    Group {
        members: Vec<FixtureInstanceId>,
    },
}

/// Select an entire fixture or group. There is no effect pixel-range selector.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FixtureTarget {
    pub layout: LayoutId,
    pub fixture: FixtureInstanceId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutError {
    TooManyPixels(FixtureInstanceId),
    DuplicateId(FixtureInstanceId),
    InvalidName(FixtureInstanceId),
    DuplicateName(crate::dsl::Identifier),
    MissingDefinition(FixtureDefinitionId),
    InvalidFixture {
        fixture: FixtureInstanceId,
        error: FixtureGeometryError,
    },
    InvalidTransform(FixtureInstanceId),
    MissingFixture(FixtureInstanceId),
    WrongLayout(LayoutId),
    /// A member is listed twice in one group or in the root.
    DuplicateMember(FixtureInstanceId),
    /// A group contains itself through its members.
    Cycle(FixtureInstanceId),
    /// An item is not reachable from the root.
    Unreachable(FixtureInstanceId),
}

impl LayoutFixture {
    pub fn members(&self) -> &[FixtureInstanceId] {
        match &self.kind {
            LayoutFixtureKind::Group { members } => members,
            LayoutFixtureKind::Fixture { .. } => &[],
        }
    }
}

impl Layout {
    /// Every fixture and group once, in authored order.
    pub fn iter_fixtures(&self) -> std::slice::Iter<'_, LayoutFixture> {
        self.fixtures.iter()
    }

    pub fn fixture(&self, id: FixtureInstanceId) -> Option<&LayoutFixture> {
        self.fixtures.iter().find(|fixture| fixture.id == id)
    }

    pub fn fixture_mut(&mut self, id: FixtureInstanceId) -> Option<&mut LayoutFixture> {
        self.fixtures.iter_mut().find(|fixture| fixture.id == id)
    }

    /// The members of a group, or the root for `None`.
    pub fn children(&self, parent: Option<FixtureInstanceId>) -> Option<&[FixtureInstanceId]> {
        match parent {
            None => Some(&self.root),
            Some(parent) => match &self.fixture(parent)?.kind {
                LayoutFixtureKind::Group { members } => Some(members),
                LayoutFixtureKind::Fixture { .. } => None,
            },
        }
    }

    /// Whether `descendant` is `ancestor` or reachable through its members.
    pub fn contains(&self, ancestor: FixtureInstanceId, descendant: FixtureInstanceId) -> bool {
        let mut seen = HashSet::new();
        let mut stack = vec![ancestor];
        while let Some(id) = stack.pop() {
            if id == descendant {
                return true;
            }
            if seen.insert(id)
                && let Some(fixture) = self.fixture(id)
            {
                stack.extend(fixture.members().iter().copied());
            }
        }
        false
    }

    /// The placed fixtures a target denotes, depth-first in member order.
    /// A fixture reached more than once keeps its first position.
    pub fn members(&self, id: FixtureInstanceId) -> Vec<FixtureInstanceId> {
        fn visit(
            index: &std::collections::HashMap<FixtureInstanceId, &LayoutFixture>,
            id: FixtureInstanceId,
            seen: &mut HashSet<FixtureInstanceId>,
            members: &mut Vec<FixtureInstanceId>,
        ) {
            if !seen.insert(id) {
                return;
            }
            let Some(fixture) = index.get(&id) else {
                return;
            };
            match &fixture.kind {
                LayoutFixtureKind::Fixture { .. } => members.push(id),
                LayoutFixtureKind::Group { members: children } => {
                    for &child in children {
                        visit(index, child, seen, members);
                    }
                }
            }
        }
        let index = self.index();
        let mut members = Vec::new();
        visit(&index, id, &mut HashSet::new(), &mut members);
        members
    }

    fn index(&self) -> std::collections::HashMap<FixtureInstanceId, &LayoutFixture> {
        self.fixtures
            .iter()
            .map(|fixture| (fixture.id, fixture))
            .collect()
    }

    pub fn target_pixel_count(
        &self,
        target: &FixtureTarget,
        counts: &IndexMap<FixtureDefinitionId, u32>,
    ) -> Result<u32, LayoutError> {
        if target.layout != self.id {
            return Err(LayoutError::WrongLayout(target.layout.clone()));
        }
        if self.fixture(target.fixture).is_none() {
            return Err(LayoutError::MissingFixture(target.fixture));
        }
        self.members(target.fixture)
            .into_iter()
            .try_fold(0u32, |total, id| {
                let fixture = self.fixture(id).ok_or(LayoutError::MissingFixture(id))?;
                let LayoutFixtureKind::Fixture { definition, .. } = &fixture.kind else {
                    return Ok(total);
                };
                let count = match definition {
                    FixtureSource::Inline(value) => {
                        value
                            .validate_geometry()
                            .map_err(|error| LayoutError::InvalidFixture {
                                fixture: fixture.id,
                                error,
                            })?
                    }
                    FixtureSource::Reference(id) => counts
                        .get(id)
                        .copied()
                        .ok_or_else(|| LayoutError::MissingDefinition(id.clone()))?,
                };
                total
                    .checked_add(count)
                    .ok_or(LayoutError::TooManyPixels(target.fixture))
            })
    }

    pub fn validate<T>(
        &self,
        definitions: &IndexMap<FixtureDefinitionId, T>,
    ) -> Result<(), LayoutError> {
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for fixture in &self.fixtures {
            if !ids.insert(fixture.id) {
                return Err(LayoutError::DuplicateId(fixture.id));
            }
            if !crate::names::is_object_name(fixture.name.as_str()) {
                return Err(LayoutError::InvalidName(fixture.id));
            }
            if !names.insert(fixture.name.clone()) {
                return Err(LayoutError::DuplicateName(fixture.name.clone()));
            }
            if let LayoutFixtureKind::Fixture {
                definition,
                transform,
            } = &fixture.kind
            {
                match definition {
                    FixtureSource::Inline(value) => {
                        value
                            .validate_geometry()
                            .map_err(|error| LayoutError::InvalidFixture {
                                fixture: fixture.id,
                                error,
                            })?;
                    }
                    FixtureSource::Reference(id) if !definitions.contains_key(id) => {
                        return Err(LayoutError::MissingDefinition(id.clone()));
                    }
                    FixtureSource::Reference(_) => {}
                }
                if !transform.is_valid() {
                    return Err(LayoutError::InvalidTransform(fixture.id));
                }
            }
        }
        self.validate_membership()
    }

    /// Members name existing items once per list, groups are acyclic, and
    /// every item is reachable from the root.
    pub fn validate_membership(&self) -> Result<(), LayoutError> {
        let index = self.index();
        let lists =
            std::iter::once(&self.root[..]).chain(self.fixtures.iter().map(LayoutFixture::members));
        for list in lists {
            let mut listed = HashSet::new();
            for &member in list {
                if !index.contains_key(&member) {
                    return Err(LayoutError::MissingFixture(member));
                }
                if !listed.insert(member) {
                    return Err(LayoutError::DuplicateMember(member));
                }
            }
        }
        // Depth-first colouring: a member still on the stack closes a cycle.
        #[derive(Clone, Copy, PartialEq)]
        enum Visit {
            Active,
            Done,
        }
        fn visit(
            index: &std::collections::HashMap<FixtureInstanceId, &LayoutFixture>,
            id: FixtureInstanceId,
            state: &mut IndexMap<FixtureInstanceId, Visit>,
        ) -> Result<(), LayoutError> {
            match state.get(&id) {
                Some(Visit::Done) => return Ok(()),
                Some(Visit::Active) => return Err(LayoutError::Cycle(id)),
                None => {}
            }
            state.insert(id, Visit::Active);
            if let Some(fixture) = index.get(&id) {
                for &member in fixture.members() {
                    visit(index, member, state)?;
                }
            }
            state.insert(id, Visit::Done);
            Ok(())
        }
        let mut state = IndexMap::new();
        for &id in &self.root {
            visit(&index, id, &mut state)?;
        }
        for fixture in &self.fixtures {
            if !state.contains_key(&fixture.id) {
                // Unreachable items may still form a cycle; report that first.
                visit(&index, fixture.id, &mut IndexMap::new())?;
                return Err(LayoutError::Unreachable(fixture.id));
            }
        }
        Ok(())
    }
}

impl crate::ownership::Identified for Layout {
    type Id = LayoutId;
    fn id(&self) -> &Self::Id {
        &self.id
    }
}

pub type LayoutSource = crate::ownership::ValueSource<Box<Layout>, LayoutId>;

impl AsRef<ObjectIdentity> for LayoutId {
    fn as_ref(&self) -> &ObjectIdentity {
        &self.0
    }
}
