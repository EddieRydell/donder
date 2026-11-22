//! Layout fixtures are effect targets. Parts inside a definition are not layout targets.
//!
//! Each fixture is placed once. Groups are ordered member lists that may share
//! members, so membership forms an acyclic graph rooted at `Layout::root`. A
//! group member may also be a slice: a run of one placed fixture's pixels.

mod editing;

use std::collections::HashSet;

use crate::fixture::{FixtureDefinitionId, FixtureGeometryError, FixtureTransform, OwnedFixture};
pub(crate) use crate::identity::LayoutId;
use crate::patch::PixelSpan;
use indexmap::IndexMap;

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
    pub name: donder_runtime_types::Identifier,
    pub description: Option<String>,
    pub kind: LayoutFixtureKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LayoutFixtureKind {
    Fixture {
        definition: OwnedFixture,
        transform: FixtureTransform,
    },
    Group {
        members: Vec<GroupMember>,
    },
}

/// A group member: a fixture or group of the layout, or a run of a placed
/// fixture's pixels in fixture order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum GroupMember {
    Item(FixtureInstanceId),
    Slice {
        fixture: FixtureInstanceId,
        pixels: PixelSpan,
    },
}

impl GroupMember {
    /// The layout item the member names: the item, or the sliced fixture.
    pub fn item(&self) -> FixtureInstanceId {
        match *self {
            Self::Item(item) | Self::Slice { fixture: item, .. } => item,
        }
    }

    /// The listed item, unless the member is a slice.
    pub fn listed(&self) -> Option<FixtureInstanceId> {
        match *self {
            Self::Item(item) => Some(item),
            Self::Slice { .. } => None,
        }
    }
}

/// Pixels a target denotes in one placed fixture: all of them, or a span.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TargetPart {
    pub fixture: FixtureInstanceId,
    pub pixels: Option<PixelSpan>,
}

/// Select an entire fixture or group; a group may list slices of fixtures.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FixtureTarget {
    pub layout: LayoutId,
    pub fixture: FixtureInstanceId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutError {
    TooManyPixels(FixtureInstanceId),
    DuplicateId(FixtureInstanceId),
    InvalidName(FixtureInstanceId, donder_runtime_types::Identifier),
    DuplicateName(FixtureInstanceId, donder_runtime_types::Identifier),
    MissingDefinition(FixtureDefinitionId),
    InvalidFixture {
        fixture: FixtureInstanceId,
        error: FixtureGeometryError,
    },
    InvalidTransform(FixtureInstanceId),
    MissingFixture(FixtureInstanceId),
    WrongLayout(LayoutId),
    /// A member is listed twice in one group or in the root.
    DuplicateMember(FixtureInstanceId, donder_runtime_types::Identifier),
    /// A group contains itself through its members.
    Cycle(FixtureInstanceId, donder_runtime_types::Identifier),
    /// An item is not reachable from the root.
    Unreachable(FixtureInstanceId, donder_runtime_types::Identifier),
    /// A group slices a group or a missing item; slices take a placed fixture.
    InvalidSlice {
        group: FixtureInstanceId,
        fixture: FixtureInstanceId,
    },
    /// A group's slice selects no pixels, or pixels past its fixture's last.
    SliceOutOfRange {
        group: FixtureInstanceId,
        fixture: FixtureInstanceId,
    },
}

impl LayoutFixture {
    /// A group's members; a fixture has none.
    pub fn members(&self) -> &[GroupMember] {
        match &self.kind {
            LayoutFixtureKind::Group { members } => members,
            LayoutFixtureKind::Fixture { .. } => &[],
        }
    }

    /// The items a group lists whole, without its slices.
    pub fn listed(&self) -> impl Iterator<Item = FixtureInstanceId> + '_ {
        self.members().iter().filter_map(GroupMember::listed)
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

    /// The items a group lists whole, or the root for `None`.
    pub fn children(&self, parent: Option<FixtureInstanceId>) -> Option<Vec<FixtureInstanceId>> {
        match parent {
            None => Some(self.root.clone()),
            Some(parent) => match &self.fixture(parent)?.kind {
                LayoutFixtureKind::Group { members } => {
                    Some(members.iter().filter_map(GroupMember::listed).collect())
                }
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
                stack.extend(fixture.members().iter().map(GroupMember::item));
            }
        }
        false
    }

    /// The placed fixtures, or runs of their pixels, a target denotes,
    /// depth-first in member order. A whole fixture or slice reached more than
    /// once keeps its first position; a pixel both parts select belongs to
    /// the first.
    pub fn members(&self, id: FixtureInstanceId) -> Vec<TargetPart> {
        fn visit(
            index: &std::collections::HashMap<FixtureInstanceId, &LayoutFixture>,
            member: GroupMember,
            seen: &mut HashSet<GroupMember>,
            parts: &mut Vec<TargetPart>,
        ) {
            if !seen.insert(member) {
                return;
            }
            match member {
                GroupMember::Slice { fixture, pixels } => parts.push(TargetPart {
                    fixture,
                    pixels: Some(pixels),
                }),
                GroupMember::Item(id) => match index.get(&id).map(|fixture| &fixture.kind) {
                    Some(LayoutFixtureKind::Fixture { .. }) => parts.push(TargetPart {
                        fixture: id,
                        pixels: None,
                    }),
                    Some(LayoutFixtureKind::Group { members }) => {
                        for &child in members {
                            visit(index, child, seen, parts);
                        }
                    }
                    None => {}
                },
            }
        }
        let index = self.index();
        let mut parts = Vec::new();
        visit(
            &index,
            GroupMember::Item(id),
            &mut HashSet::new(),
            &mut parts,
        );
        parts
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
            return Err(LayoutError::WrongLayout(target.layout));
        }
        if self.fixture(target.fixture).is_none() {
            return Err(LayoutError::MissingFixture(target.fixture));
        }
        // Each placed fixture's selected ranges, merged so a pixel counts once.
        let mut selected: IndexMap<FixtureInstanceId, Vec<(u32, u32)>> = IndexMap::new();
        for part in self.members(target.fixture) {
            let count = self.fixture_pixel_count(part.fixture, counts)?;
            let range = part.pixels.map_or((0, count), |span| {
                (
                    span.start.min(count),
                    span.start.saturating_add(span.count).min(count),
                )
            });
            selected.entry(part.fixture).or_default().push(range);
        }
        selected.into_values().try_fold(0u32, |total, mut ranges| {
            ranges.sort_unstable();
            let mut covered = 0u32;
            let mut end = 0u32;
            for (start, stop) in ranges {
                let start = start.max(end);
                if stop > start {
                    covered += stop - start;
                    end = stop;
                }
            }
            total
                .checked_add(covered)
                .ok_or(LayoutError::TooManyPixels(target.fixture))
        })
    }

    /// A placed fixture's pixel count; a group has none of its own.
    pub fn fixture_pixel_count(
        &self,
        id: FixtureInstanceId,
        counts: &IndexMap<FixtureDefinitionId, u32>,
    ) -> Result<u32, LayoutError> {
        let fixture = self.fixture(id).ok_or(LayoutError::MissingFixture(id))?;
        let LayoutFixtureKind::Fixture { definition, .. } = &fixture.kind else {
            return Ok(0);
        };
        match definition {
            OwnedFixture::Inline(value) => {
                value
                    .validate_geometry()
                    .map_err(|error| LayoutError::InvalidFixture {
                        fixture: fixture.id,
                        error,
                    })
            }
            OwnedFixture::Reference(definition) => counts
                .get(definition)
                .copied()
                .ok_or(LayoutError::MissingDefinition(*definition)),
        }
    }

    /// Every slice selects pixels its fixture has, given each reusable
    /// definition's pixel count.
    pub fn validate_slices(
        &self,
        counts: &IndexMap<FixtureDefinitionId, u32>,
    ) -> Result<(), LayoutError> {
        for group in &self.fixtures {
            for member in group.members() {
                let GroupMember::Slice { fixture, pixels } = *member else {
                    continue;
                };
                let count = self.fixture_pixel_count(fixture, counts)?;
                if pixels
                    .start
                    .checked_add(pixels.count)
                    .is_none_or(|end| end > count)
                {
                    return Err(LayoutError::SliceOutOfRange {
                        group: group.id,
                        fixture,
                    });
                }
            }
        }
        Ok(())
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
            if !donder_language::NameKind::Object.accepts(fixture.name.as_str()) {
                return Err(LayoutError::InvalidName(fixture.id, fixture.name.clone()));
            }
            if !names.insert(fixture.name.clone()) {
                return Err(LayoutError::DuplicateName(fixture.id, fixture.name.clone()));
            }
            if let LayoutFixtureKind::Fixture {
                definition,
                transform,
            } = &fixture.kind
            {
                match definition {
                    OwnedFixture::Inline(value) => {
                        value
                            .validate_geometry()
                            .map_err(|error| LayoutError::InvalidFixture {
                                fixture: fixture.id,
                                error,
                            })?;
                    }
                    OwnedFixture::Reference(id) if !definitions.contains_key(id) => {
                        return Err(LayoutError::MissingDefinition(*id));
                    }
                    OwnedFixture::Reference(_) => {}
                }
                if !transform.is_valid() {
                    return Err(LayoutError::InvalidTransform(fixture.id));
                }
            }
        }
        self.validate_membership()
    }

    /// Members name existing items once per list, slices name placed
    /// fixtures and select pixels, groups are acyclic, and every item is
    /// reachable from the root.
    fn validate_membership(&self) -> Result<(), LayoutError> {
        let index = self.index();
        let root = self
            .root
            .iter()
            .copied()
            .map(GroupMember::Item)
            .collect::<Vec<_>>();
        let lists = std::iter::once((None, &root[..])).chain(
            self.fixtures
                .iter()
                .map(|fixture| (Some(fixture.id), fixture.members())),
        );
        for (group, list) in lists {
            let mut listed = HashSet::new();
            for &member in list {
                let item = member.item();
                let Some(fixture) = index.get(&item) else {
                    return Err(LayoutError::MissingFixture(item));
                };
                if let (GroupMember::Slice { pixels, .. }, Some(group)) = (member, group) {
                    if !matches!(fixture.kind, LayoutFixtureKind::Fixture { .. }) {
                        return Err(LayoutError::InvalidSlice {
                            group,
                            fixture: item,
                        });
                    }
                    if pixels.count == 0 {
                        return Err(LayoutError::SliceOutOfRange {
                            group,
                            fixture: item,
                        });
                    }
                }
                if !listed.insert(member) {
                    return Err(LayoutError::DuplicateMember(item, fixture.name.clone()));
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
                Some(Visit::Active) => {
                    return Err(LayoutError::Cycle(id, index[&id].name.clone()));
                }
                None => {}
            }
            state.insert(id, Visit::Active);
            if let Some(fixture) = index.get(&id) {
                for member in fixture.members() {
                    visit(index, member.item(), state)?;
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
                return Err(LayoutError::Unreachable(fixture.id, fixture.name.clone()));
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

pub type OwnedLayout = crate::ownership::Owned<Box<Layout>, LayoutId>;
