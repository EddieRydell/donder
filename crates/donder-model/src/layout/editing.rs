use super::{FixtureInstanceId, GroupMember, Layout, LayoutFixtureKind};
use crate::project::EditRejection;

/// The root's items, or a group's members, for editing the items it lists.
enum MembersMut<'a> {
    Root(&'a mut Vec<FixtureInstanceId>),
    Group(&'a mut Vec<GroupMember>),
}

impl MembersMut<'_> {
    /// Where the list names `id` whole.
    fn position(&self, id: FixtureInstanceId) -> Option<usize> {
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

    fn insert(&mut self, index: usize, id: FixtureInstanceId) {
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
    pub fn parents(&self, id: FixtureInstanceId) -> Vec<Option<FixtureInstanceId>> {
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
    fn listed_anywhere(&self, id: FixtureInstanceId) -> bool {
        self.root.contains(&id)
            || self
                .fixtures
                .iter()
                .any(|fixture| fixture.members().iter().any(|member| member.item() == id))
    }

    fn members_mut(
        &mut self,
        parent: Option<FixtureInstanceId>,
    ) -> Result<MembersMut<'_>, EditRejection> {
        match parent {
            None => Ok(MembersMut::Root(&mut self.root)),
            Some(parent) => match &mut self
                .fixture_mut(parent)
                .ok_or(EditRejection::MissingItem)?
                .kind
            {
                LayoutFixtureKind::Group { members } => Ok(MembersMut::Group(members)),
                LayoutFixtureKind::Fixture { .. } => Err(EditRejection::NotAGroup),
            },
        }
    }

    /// Move a member from one group to another without changing its identity
    /// or placement. `None` is the root; `before` is a member of `to`, and
    /// `None` appends. Moving within one group reorders it.
    pub fn move_member(
        &mut self,
        id: FixtureInstanceId,
        from: Option<FixtureInstanceId>,
        to: Option<FixtureInstanceId>,
        before: Option<FixtureInstanceId>,
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
        id: FixtureInstanceId,
        to: Option<FixtureInstanceId>,
        before: Option<FixtureInstanceId>,
    ) -> Result<(), EditRejection> {
        self.check_insert(id, to, before)?;
        self.insert(id, to, before)
    }

    /// List a new item right after `original` in every group listing it.
    pub(crate) fn add_beside(&mut self, id: FixtureInstanceId, original: FixtureInstanceId) {
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
        id: FixtureInstanceId,
        from: Option<FixtureInstanceId>,
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
    pub fn remove_items(&mut self, ids: &[FixtureInstanceId]) -> Result<(), EditRejection> {
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
        id: FixtureInstanceId,
        to: Option<FixtureInstanceId>,
        before: Option<FixtureInstanceId>,
    ) -> Result<(), EditRejection> {
        if self.fixture(id).is_none() {
            return Err(EditRejection::MissingItem);
        }
        let destination = self.children(to).ok_or(EditRejection::NotAGroup)?;
        if destination.contains(&id) {
            return Err(EditRejection::AlreadyInGroup);
        }
        if to.is_some_and(|to| self.contains(id, to)) {
            return Err(EditRejection::GroupInsideItself);
        }
        if before.is_some_and(|before| !destination.contains(&before)) {
            return Err(EditRejection::InsertionTargetOutside);
        }
        Ok(())
    }

    fn insert(
        &mut self,
        id: FixtureInstanceId,
        to: Option<FixtureInstanceId>,
        before: Option<FixtureInstanceId>,
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
