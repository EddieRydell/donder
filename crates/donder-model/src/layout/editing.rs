use super::{FixtureInstanceId, Layout, LayoutFixtureKind};
use crate::project::EditRejection;

impl Layout {
    /// The groups listing an item, with `None` for the root.
    pub fn parents(&self, id: FixtureInstanceId) -> Vec<Option<FixtureInstanceId>> {
        std::iter::once((None, &self.root[..]))
            .chain(
                self.fixtures
                    .iter()
                    .map(|fixture| (Some(fixture.id), fixture.members())),
            )
            .filter(|(_, members)| members.contains(&id))
            .map(|(parent, _)| parent)
            .collect()
    }

    fn members_mut(
        &mut self,
        parent: Option<FixtureInstanceId>,
    ) -> Result<&mut Vec<FixtureInstanceId>, EditRejection> {
        match parent {
            None => Ok(&mut self.root),
            Some(parent) => match &mut self
                .fixture_mut(parent)
                .ok_or(EditRejection::MissingItem)?
                .kind
            {
                LayoutFixtureKind::Group { members } => Ok(members),
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
        } else if before.is_some_and(|before| !self.children(to).unwrap_or(&[]).contains(&before)) {
            return Err(EditRejection::InsertionTargetOutside);
        }
        self.members_mut(from)?.retain(|member| *member != id);
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
        let lists =
            std::iter::once(&mut self.root).chain(self.fixtures.iter_mut().filter_map(|fixture| {
                match &mut fixture.kind {
                    LayoutFixtureKind::Group { members } => Some(members),
                    LayoutFixtureKind::Fixture { .. } => None,
                }
            }));
        for members in lists {
            if let Some(index) = members.iter().position(|member| *member == original) {
                members.insert(index + 1, id);
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
        let members = self.members_mut(from)?;
        let Some(index) = members.iter().position(|member| *member == id) else {
            return Err(EditRejection::NotInGroup);
        };
        members.remove(index);
        if self.parents(id).is_empty() {
            self.root.push(id);
        }
        Ok(())
    }

    /// Delete items everywhere. Members of a deleted group that no remaining
    /// group lists move to the root.
    pub fn remove_items(&mut self, ids: &[FixtureInstanceId]) -> Result<(), EditRejection> {
        let mut orphans = Vec::new();
        for &id in ids {
            let fixture = self.fixture(id).ok_or(EditRejection::MissingItem)?;
            orphans.extend(fixture.members().iter().copied());
        }
        self.fixtures.retain(|fixture| !ids.contains(&fixture.id));
        self.root.retain(|member| !ids.contains(member));
        for fixture in &mut self.fixtures {
            if let LayoutFixtureKind::Group { members } = &mut fixture.kind {
                members.retain(|member| !ids.contains(member));
            }
        }
        for orphan in orphans {
            if !ids.contains(&orphan) && self.parents(orphan).is_empty() {
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
        let destination = self.members_mut(to)?;
        let index = match before {
            Some(before) => destination
                .iter()
                .position(|member| *member == before)
                .ok_or(EditRejection::InsertionTargetOutside)?,
            None => destination.len(),
        };
        destination.insert(index, id);
        Ok(())
    }
}
