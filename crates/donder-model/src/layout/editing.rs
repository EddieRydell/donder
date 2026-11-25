use super::{FixtureId, GroupMember, Layout, LayoutFixtureKind};
use crate::project::EditRejection;

/// The root's items, or a group's members, for editing the items it lists.
enum MembersMut<'a> {
    Root(&'a mut Vec<FixtureId>),
    Group(&'a mut Vec<GroupMember>),
}

impl MembersMut<'_> {
    /// Where the list names `id` whole.
    fn position(&self, id: FixtureId) -> Option<usize> {
        match self {
            Self::Root(items) => items.iter().position(|item| *item == id),
            Self::Group(members) => members
                .iter()
                .position(|member| *member == GroupMember::Item(id)),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Root(items) => items.len(),
            Self::Group(members) => members.len(),
        }
    }

    fn insert(&mut self, index: usize, id: FixtureId) {
        match self {
            Self::Root(items) => items.insert(index, id),
            Self::Group(members) => members.insert(index, GroupMember::Item(id)),
        }
    }

    fn remove(&mut self, index: usize) {
        match self {
            Self::Root(items) => {
                items.remove(index);
            }
            Self::Group(members) => {
                members.remove(index);
            }
        }
    }
}

impl Layout {
    /// The groups listing an item whole, with `None` for the root.
    pub fn parents(&self, id: FixtureId) -> Vec<Option<FixtureId>> {
        let mut parents = Vec::new();
        if self.root.contains(&id) {
            parents.push(None);
        }
        parents.extend(
            self.fixtures
                .iter()
                .filter(|fixture| fixture.members().contains(&GroupMember::Item(id)))
                .map(|fixture| Some(fixture.id)),
        );
        parents
    }

    /// Whether any group, or the root, names the item, whole or by a slice.
    fn listed_anywhere(&self, id: FixtureId) -> bool {
        self.root.contains(&id)
            || self
                .fixtures
                .iter()
                .any(|fixture| fixture.members().iter().any(|member| member.item() == id))
    }

    fn members_mut(&mut self, parent: Option<FixtureId>) -> Result<MembersMut<'_>, EditRejection> {
        match parent {
            None => Ok(MembersMut::Root(&mut self.root)),
            Some(parent) => match &mut self
                .fixture_mut(parent)
                .ok_or(EditRejection::MissingItem)?
                .kind
            {
                LayoutFixtureKind::Group { members } => Ok(MembersMut::Group(members)),
                LayoutFixtureKind::Fixture { .. } => Err(EditRejection::MissingItem),
            },
        }
    }

    /// Move a member from one group to another without changing its identity
    /// or placement. `None` is the root; `before` is a member of `to`, and
    /// `None` appends. Moving within one group reorders it.
    pub fn move_member(
        &mut self,
        id: FixtureId,
        from: Option<FixtureId>,
        to: Option<FixtureId>,
        before: Option<FixtureId>,
    ) -> Result<(), EditRejection> {
        if !self
            .children(from)
            .is_some_and(|members| members.contains(&id))
        {
            return Err(EditRejection::NotInSourceGroup);
        }
        if from != to {
            self.check_insert(id, to, before)?;
        } else if before == Some(id) {
            return Ok(());
        } else if before
            .is_some_and(|before| !self.children(to).unwrap_or_default().contains(&before))
        {
            return Err(EditRejection::InsertionTargetOutside);
        }
        let mut source = self.members_mut(from)?;
        if let Some(index) = source.position(id) {
            source.remove(index);
        }
        self.insert(id, to, before)
    }

    /// List an existing item in another group as well.
    pub fn add_member(
        &mut self,
        id: FixtureId,
        to: Option<FixtureId>,
        before: Option<FixtureId>,
    ) -> Result<(), EditRejection> {
        self.check_insert(id, to, before)?;
        self.insert(id, to, before)
    }

    /// List a new item right after `original` in every group listing it.
    pub(crate) fn add_beside(&mut self, id: FixtureId, original: FixtureId) {
        if let Some(index) = self.root.iter().position(|member| *member == original) {
            self.root.insert(index + 1, id);
        }
        for fixture in &mut self.fixtures {
            if let LayoutFixtureKind::Group { members } = &mut fixture.kind
                && let Some(index) = members
                    .iter()
                    .position(|member| *member == GroupMember::Item(original))
            {
                members.insert(index + 1, GroupMember::Item(id));
            }
        }
    }

    /// Remove an item from one group. An item left without any group moves to
    /// the root so it stays reachable.
    pub fn remove_member(
        &mut self,
        id: FixtureId,
        from: Option<FixtureId>,
    ) -> Result<(), EditRejection> {
        let mut members = self.members_mut(from)?;
        let Some(index) = members.position(id) else {
            return Err(EditRejection::NotInGroup);
        };
        members.remove(index);
        if !self.listed_anywhere(id) {
            self.root.push(id);
        }
        Ok(())
    }

    /// Delete items everywhere, with every slice of them. Members of a
    /// deleted group that no remaining group lists move to the root.
    pub fn remove_items(&mut self, ids: &[FixtureId]) -> Result<(), EditRejection> {
        let mut orphans = Vec::new();
        for &id in ids {
            let fixture = self.fixture(id).ok_or(EditRejection::MissingItem)?;
            orphans.extend(fixture.members().iter().map(GroupMember::item));
        }
        self.fixtures.retain(|fixture| !ids.contains(&fixture.id));
        self.root.retain(|member| !ids.contains(member));
        for fixture in &mut self.fixtures {
            if let LayoutFixtureKind::Group { members } = &mut fixture.kind {
                members.retain(|member| !ids.contains(&member.item()));
            }
        }
        for orphan in orphans {
            if !ids.contains(&orphan) && !self.listed_anywhere(orphan) {
                self.root.push(orphan);
            }
        }
        Ok(())
    }

    fn check_insert(
        &self,
        id: FixtureId,
        to: Option<FixtureId>,
        before: Option<FixtureId>,
    ) -> Result<(), EditRejection> {
        if self.fixture(id).is_none() {
            return Err(EditRejection::MissingItem);
        }
        let destination = self.children(to).ok_or(EditRejection::MissingItem)?;
        if before.is_some_and(|before| !destination.contains(&before)) {
            return Err(EditRejection::InsertionTargetOutside);
        }
        Ok(())
    }

    fn insert(
        &mut self,
        id: FixtureId,
        to: Option<FixtureId>,
        before: Option<FixtureId>,
    ) -> Result<(), EditRejection> {
        let mut destination = self.members_mut(to)?;
        let index = match before {
            Some(before) => destination
                .position(before)
                .ok_or(EditRejection::InsertionTargetOutside)?,
            None => destination.len(),
        };
        destination.insert(index, id);
        Ok(())
    }
}

/// The copies a duplication made: each copied fixture and the definition
/// copied for it.
pub type FixtureCopies = Vec<(FixtureId, crate::fixture::FixtureDefinitionId)>;

struct Copier<'a> {
    layout: &'a Layout,
    next: u32,
    taken: std::collections::HashSet<donder_runtime_types::Identifier>,
    copies: indexmap::IndexMap<FixtureId, super::LayoutFixture>,
}

impl Copier<'_> {
    // Every copied placement and group gets its own unique name.
    fn copy(&mut self, id: FixtureId) -> Result<FixtureId, EditRejection> {
        if let Some(copy) = self.copies.get(&id) {
            return Ok(copy.id);
        }
        let mut copy = self
            .layout
            .fixture(id)
            .ok_or(EditRejection::MissingItem)?
            .clone();
        self.next = self.next.checked_add(1).ok_or(EditRejection::NoFreeIds)?;
        copy.id = FixtureId(self.next);
        copy.name = donder_language::unique_name(&format!("{}_copy", copy.name.as_str()), |name| {
            self.taken.iter().any(|taken| taken.as_str() == name)
        });
        self.taken.insert(copy.name.clone());
        if let LayoutFixtureKind::Group { members } = &mut copy.kind {
            *members = members
                .iter()
                .map(|&member| match member {
                    GroupMember::Item(item) => self.copy(item).map(GroupMember::Item),
                    GroupMember::Slice { fixture, pixels } => self
                        .copy(fixture)
                        .map(|fixture| GroupMember::Slice { fixture, pixels }),
                })
                .collect::<Result<_, _>>()?;
        }
        let copied = copy.id;
        self.copies.insert(id, copy);
        Ok(copied)
    }
}

/// Duplicate a placement or group of the project's layout with independent
/// geometry: each copied placement gets a copy of its definition. A group
/// copies its members once each, so members shared inside it stay shared. The
/// copy is listed after the original in every group listing the original.
/// Existing routes and effects keep targeting the originals. Returns the copy
/// and the definitions copied, which the caller declares.
pub fn duplicate_layout_fixture(
    project: &mut crate::DonderProject,
    fixture_id: FixtureId,
) -> Result<(FixtureId, FixtureCopies), crate::EditError> {
    project.checked_edit(|project| {
        let layout = project.layout().object();
        let mut copier = Copier {
            layout,
            next: layout
                .iter_fixtures()
                .map(|fixture| fixture.id.0)
                .max()
                .unwrap_or(0),
            taken: layout
                .iter_fixtures()
                .map(|fixture| fixture.name.clone())
                .collect(),
            copies: indexmap::IndexMap::new(),
        };
        let id = copier.copy(fixture_id)?;
        let mut copies = copier.copies;
        let mut declared = Vec::new();
        for copy in copies.values_mut() {
            if let LayoutFixtureKind::Fixture { definition, .. } = &mut copy.kind {
                let value = project
                    .get::<crate::FixtureDefinition>(*definition)
                    .ok_or(EditRejection::MissingItem)?
                    .clone();
                *definition = crate::fixture::FixtureDefinitionId(project.new_id());
                project.fixtures.insert(*definition, value);
                declared.push((copy.id, *definition));
            }
        }
        let layout_id = project.root.layout;
        let layout = project
            .find_mut::<Layout>(layout_id)
            .ok_or(EditRejection::Missing(crate::ObjectKind::Layout))?;
        for (original, copy) in copies {
            let index = layout
                .fixtures
                .iter()
                .position(|fixture| fixture.id == original)
                .ok_or(EditRejection::MissingItem)?;
            layout.fixtures.insert(index + 1, copy);
        }
        layout.add_beside(id, fixture_id);
        Ok((id, declared))
    })
}
