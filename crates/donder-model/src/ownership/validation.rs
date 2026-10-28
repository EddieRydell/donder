use super::{Identified, Owned, OwnedMember};
use crate::identity::ObjectId;
use crate::project::DonderProject;
use crate::validation::{OwnershipProblem, ProjectValidationError, ValidationProblem, at};
use indexmap::IndexMap;
use std::collections::HashSet;

/// Every object id once: reusable values under their own ids, inline values
/// under ids no other object uses.
struct Ids(HashSet<ObjectId>);

impl Ids {
    fn insert(&mut self, id: impl Into<ObjectId>) -> Result<(), ValidationProblem> {
        let id = id.into();
        if self.0.insert(id) {
            Ok(())
        } else {
            Err(ValidationProblem::Ownership(OwnershipProblem::DuplicateId(
                id,
            )))
        }
    }

    fn store<T: Identified<Id = I>, I: Copy + Eq + Into<ObjectId>>(
        &mut self,
        values: &IndexMap<I, T>,
    ) -> Result<(), ValidationProblem> {
        for (id, value) in values {
            if id != value.id() {
                return Err(ValidationProblem::Ownership(
                    OwnershipProblem::StoredUnderOtherId((*id).into()),
                ));
            }
            self.insert(*id)?;
        }
        Ok(())
    }

    fn value<T: Identified<Id = I>, I: Copy + Into<ObjectId>>(
        &mut self,
        source: &Owned<Box<T>, I>,
        exists: impl FnOnce(&I) -> bool,
    ) -> Result<(), ValidationProblem> {
        match source {
            Owned::Inline(value) => self.insert(*value.id()),
            Owned::Reference(id) => reference(exists(id), *id),
        }
    }

    fn member<T: Identified<Id = I>, I: Copy + Into<ObjectId>>(
        &mut self,
        source: &OwnedMember<T, I>,
        exists: impl FnOnce(&I) -> bool,
    ) -> Result<(), ValidationProblem> {
        match source {
            OwnedMember::Inline { value, .. } => self.insert(*value.id()),
            OwnedMember::Reference(id) => reference(exists(id), *id),
        }
    }
}

fn reference(exists: bool, id: impl Into<ObjectId>) -> Result<(), ValidationProblem> {
    if exists {
        Ok(())
    } else {
        Err(ValidationProblem::Ownership(
            OwnershipProblem::MissingReference(id.into()),
        ))
    }
}

/// Members of one collection are distinct values with distinct names.
fn distinct_members<T: Identified<Id = I>, I: Eq + std::hash::Hash>(
    sources: &[OwnedMember<T, I>],
    problem: OwnershipProblem,
) -> Result<(), ValidationProblem> {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for source in sources {
        if !ids.insert(source.id()) || source.name().is_some_and(|name| !names.insert(name)) {
            return Err(ValidationProblem::Ownership(problem));
        }
    }
    Ok(())
}

pub(crate) fn validate_ownership(project: &DonderProject) -> Result<(), ProjectValidationError> {
    let mut ids = Ids(HashSet::new());
    ids.insert(project.root.id).map_err(at(project.root.id))?;
    ids.store(&project.setups).map_err(at(project.root.id))?;
    ids.store(&project.layouts).map_err(at(project.root.id))?;
    ids.store(&project.patches).map_err(at(project.root.id))?;
    ids.store(&project.controllers)
        .map_err(at(project.root.id))?;
    ids.store(&project.input_devices)
        .map_err(at(project.root.id))?;
    ids.store(&project.sequences).map_err(at(project.root.id))?;
    ids.store(&project.rigs).map_err(at(project.root.id))?;
    ids.value(&project.root.setup, |id| project.setups.contains_key(id))
        .map_err(at(project.root.id))?;
    distinct_members(&project.root.sequences, OwnershipProblem::DuplicateSequence)
        .map_err(at(project.root.id))?;
    for source in &project.root.sequences {
        ids.member(source, |id| project.sequences.contains_key(id))
            .map_err(at(project.root.id))?;
    }
    distinct_members(&project.root.rigs, OwnershipProblem::DuplicateRig)
        .map_err(at(project.root.id))?;
    for source in &project.root.rigs {
        ids.member(source, |id| project.rigs.contains_key(id))
            .map_err(at(project.root.id))?;
    }
    for setup in project.objects::<crate::Setup>() {
        ids.value(&setup.layout, |id| project.layouts.contains_key(id))
            .map_err(at(setup.id))?;
        ids.value(&setup.patch, |id| project.patches.contains_key(id))
            .map_err(at(setup.id))?;
        distinct_members(&setup.controllers, OwnershipProblem::DuplicateController)
            .map_err(at(setup.id))?;
        for source in &setup.controllers {
            ids.member(source, |id| project.controllers.contains_key(id))
                .map_err(at(setup.id))?;
        }
        distinct_members(&setup.input_devices, OwnershipProblem::DuplicateInputDevice)
            .map_err(at(setup.id))?;
        for source in &setup.input_devices {
            ids.member(source, |id| project.input_devices.contains_key(id))
                .map_err(at(setup.id))?;
        }
    }
    Ok(())
}
