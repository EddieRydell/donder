use super::{Identified, ValueSource};
use crate::identity::{ObjectIdentity, OwnedObjectSlot};
use crate::model::DonderProject;
use crate::validation::ProjectValidationError;
use indexmap::IndexMap;
use std::collections::HashSet;

fn invalid(message: &str) -> ProjectValidationError {
    ProjectValidationError::InvalidRelationship(message.into())
}

fn validate_named<T: Identified<Id = I>, I: AsRef<ObjectIdentity> + Eq>(
    values: &IndexMap<I, T>,
) -> Result<(), ProjectValidationError> {
    for (id, value) in values {
        if id.as_ref().source().is_none() || id != value.id() {
            return Err(invalid(
                "Reusable objects require matching named source identities.",
            ));
        }
    }
    Ok(())
}

enum ExpectedSlot {
    Setup,
    Layout,
    Patch,
    Controller,
    Sequence,
}

impl ExpectedSlot {
    fn matches(&self, actual: &OwnedObjectSlot) -> bool {
        matches!(
            (self, actual),
            (Self::Setup, OwnedObjectSlot::Setup)
                | (Self::Layout, OwnedObjectSlot::Layout)
                | (Self::Patch, OwnedObjectSlot::Patch)
                | (Self::Controller, OwnedObjectSlot::Controller(_))
                | (Self::Sequence, OwnedObjectSlot::Sequence(_))
        )
    }
}

fn validate_source<T: Identified<Id = I>, I: AsRef<ObjectIdentity>>(
    source: &ValueSource<Box<T>, I>,
    owner: &ObjectIdentity,
    expected: ExpectedSlot,
    exists: impl FnOnce(&I) -> bool,
) -> Result<(), ProjectValidationError> {
    match source {
        ValueSource::Inline(value) => {
            let identity = value.id().as_ref();
            let Some(slot) = identity.owned_path().last() else {
                return Err(invalid("Inline objects require an ownership slot."));
            };
            if !expected.matches(slot) || identity != &owner.owned(slot.clone()) {
                return Err(invalid("Inline object address does not match its owner."));
            }
        }
        ValueSource::Reference(id) => {
            if id.as_ref().source().is_none() {
                return Err(invalid(
                    "An owned object must be made reusable before linking it.",
                ));
            }
            if !exists(id) {
                return Err(invalid("Reusable object reference is missing."));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_ownership(project: &DonderProject) -> Result<(), ProjectValidationError> {
    validate_named(&project.setups)?;
    validate_named(&project.layouts)?;
    validate_named(&project.patches)?;
    validate_named(&project.controllers)?;
    validate_named(&project.sequences)?;
    let root = ObjectIdentity::from(project.root.id.0.clone());
    validate_source(&project.root.setup, &root, ExpectedSlot::Setup, |id| {
        project.setups.contains_key(id)
    })?;
    let mut sequences = HashSet::new();
    for source in &project.root.sequences {
        validate_source(source, &root, ExpectedSlot::Sequence, |id| {
            project.sequences.contains_key(id)
        })?;
        if !sequences.insert(source.id()) {
            return Err(invalid("Sequence appears more than once in the project."));
        }
    }
    for setup in project.setups() {
        validate_source(&setup.layout, &setup.id.0, ExpectedSlot::Layout, |id| {
            project.layouts.contains_key(id)
        })?;
        validate_source(&setup.patch, &setup.id.0, ExpectedSlot::Patch, |id| {
            project.patches.contains_key(id)
        })?;
        let mut controllers = HashSet::new();
        for source in &setup.controllers {
            validate_source(source, &setup.id.0, ExpectedSlot::Controller, |id| {
                project.controllers.contains_key(id)
            })?;
            if !controllers.insert(source.id()) {
                return Err(invalid("Controller appears more than once in the setup."));
            }
        }
    }
    Ok(())
}
