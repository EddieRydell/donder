use super::*;
use crate::effect::{
    CurveDefinition, CurveId, EffectDefinition, EffectDefinitionId, GradientDefinition, GradientId,
};
use crate::fixture::{FixtureDefinition, FixtureDefinitionId};
use crate::operator::{OperatorDefinition, OperatorDefinitionId};
use crate::sequence::SequenceId;

/// Detached authored changes. Every batch is checked as one operation, allowing
/// related values to change together without exposing a mutable project draft.
#[derive(Clone, Debug)]
pub enum ProjectEdit {
    ReplaceRoot(ProjectRoot),
    ReplaceSetup {
        id: SetupId,
        value: Setup,
    },
    ReplaceLayout {
        id: LayoutId,
        value: Layout,
    },
    ReplacePatch {
        id: PatchId,
        value: Patch,
    },
    ReplaceController {
        id: ControllerId,
        value: Controller,
    },
    ReplaceSequence {
        id: SequenceId,
        value: Sequence,
    },
    InsertSetup(Setup),
    InsertLayout(Layout),
    InsertPatch(Patch),
    InsertController(Controller),
    InsertSequence(Sequence),
    SetFixtureDefinition {
        id: FixtureDefinitionId,
        value: FixtureDefinition,
    },
    SetEffectDefinition {
        id: EffectDefinitionId,
        value: EffectDefinition,
    },
    SetOperatorDefinition {
        id: OperatorDefinitionId,
        value: OperatorDefinition,
    },
    SetCurveDefinition {
        id: CurveId,
        value: CurveDefinition,
    },
    SetGradientDefinition {
        id: GradientId,
        value: GradientDefinition,
    },
    RemoveSetup(SetupId),
    RemoveLayout(LayoutId),
    RemovePatch(PatchId),
    RemoveController(ControllerId),
    RemoveSequence(SequenceId),
    RemoveFixtureDefinition(FixtureDefinitionId),
    RemoveEffectDefinition(EffectDefinitionId),
    RemoveOperatorDefinition(OperatorDefinitionId),
    RemoveCurveDefinition(CurveId),
    RemoveGradientDefinition(GradientId),
}

fn replace<T: crate::ownership::Identified>(
    slot: Option<&mut T>,
    id: &T::Id,
    value: T,
) -> Result<(), String>
where
    T::Id: PartialEq,
{
    if value.id() != id {
        return Err("Replacement must preserve the object's identity.".into());
    }
    *slot.ok_or("Object was not found.")? = value;
    Ok(())
}

fn insert<I: Eq + std::hash::Hash + Clone, T: crate::ownership::Identified<Id = I>>(
    map: &mut IndexMap<I, T>,
    value: T,
) -> Result<(), String> {
    if map.contains_key(value.id()) {
        return Err("An object already exists at this identity.".into());
    }
    map.insert(value.id().clone(), value);
    Ok(())
}

fn remove<I: Eq + std::hash::Hash, T>(map: &mut IndexMap<I, T>, id: &I) -> Result<(), String> {
    map.shift_remove(id).ok_or("Object was not found.")?;
    Ok(())
}

impl DonderProject {
    pub fn apply_edits(
        &mut self,
        edits: impl IntoIterator<Item = ProjectEdit>,
    ) -> Result<(), String> {
        self.checked_edit(|project| {
            for edit in edits {
                match edit {
                    ProjectEdit::ReplaceRoot(value) => project.root = value.into(),
                    ProjectEdit::ReplaceSetup { id, value } => {
                        replace(project.setup_mut(&id), &id, value)?
                    }
                    ProjectEdit::ReplaceLayout { id, value } => {
                        replace(project.layout_mut(&id), &id, value)?
                    }
                    ProjectEdit::ReplacePatch { id, value } => {
                        replace(project.patch_mut(&id), &id, value)?
                    }
                    ProjectEdit::ReplaceController { id, value } => {
                        replace(project.controller_mut(&id), &id, value)?
                    }
                    ProjectEdit::ReplaceSequence { id, value } => {
                        replace(project.sequence_mut(&id), &id, value)?
                    }
                    ProjectEdit::InsertSetup(value) => insert(&mut project.setups, value)?,
                    ProjectEdit::InsertLayout(value) => insert(&mut project.layouts, value)?,
                    ProjectEdit::InsertPatch(value) => insert(&mut project.patches, value)?,
                    ProjectEdit::InsertController(value) => {
                        insert(&mut project.controllers, value)?
                    }
                    ProjectEdit::InsertSequence(value) => insert(&mut project.sequences, value)?,
                    ProjectEdit::SetFixtureDefinition { id, value } => {
                        project.definitions.fixtures.definitions.insert(id, value);
                    }
                    ProjectEdit::SetEffectDefinition { id, value } => {
                        project.definitions.effects.definitions.insert(id, value);
                    }
                    ProjectEdit::SetOperatorDefinition { id, value } => {
                        project.definitions.operators.definitions.insert(id, value);
                    }
                    ProjectEdit::SetCurveDefinition { id, value } => {
                        project.definitions.curves.definitions.insert(id, value);
                    }
                    ProjectEdit::SetGradientDefinition { id, value } => {
                        project.definitions.gradients.definitions.insert(id, value);
                    }
                    ProjectEdit::RemoveSetup(id) => remove(&mut project.setups, &id)?,
                    ProjectEdit::RemoveLayout(id) => remove(&mut project.layouts, &id)?,
                    ProjectEdit::RemovePatch(id) => remove(&mut project.patches, &id)?,
                    ProjectEdit::RemoveController(id) => remove(&mut project.controllers, &id)?,
                    ProjectEdit::RemoveSequence(id) => remove(&mut project.sequences, &id)?,
                    ProjectEdit::RemoveFixtureDefinition(id) => {
                        remove(&mut project.definitions.fixtures.definitions, &id)?
                    }
                    ProjectEdit::RemoveEffectDefinition(id) => {
                        remove(&mut project.definitions.effects.definitions, &id)?
                    }
                    ProjectEdit::RemoveOperatorDefinition(id) => {
                        remove(&mut project.definitions.operators.definitions, &id)?
                    }
                    ProjectEdit::RemoveCurveDefinition(id) => {
                        remove(&mut project.definitions.curves.definitions, &id)?
                    }
                    ProjectEdit::RemoveGradientDefinition(id) => {
                        remove(&mut project.definitions.gradients.definitions, &id)?
                    }
                }
            }
            Ok(())
        })
    }

    pub fn replace_root(&mut self, value: ProjectRoot) -> Result<(), String> {
        self.apply_edits([ProjectEdit::ReplaceRoot(value)])
    }
    pub fn replace_setup(&mut self, id: &SetupId, value: Setup) -> Result<(), String> {
        self.apply_edits([ProjectEdit::ReplaceSetup {
            id: id.clone(),
            value,
        }])
    }
    pub fn replace_layout(&mut self, id: &LayoutId, value: Layout) -> Result<(), String> {
        self.apply_edits([ProjectEdit::ReplaceLayout {
            id: id.clone(),
            value,
        }])
    }
    pub fn replace_patch(&mut self, id: &PatchId, value: Patch) -> Result<(), String> {
        self.apply_edits([ProjectEdit::ReplacePatch {
            id: id.clone(),
            value,
        }])
    }
    pub fn replace_controller(
        &mut self,
        id: &ControllerId,
        value: Controller,
    ) -> Result<(), String> {
        self.apply_edits([ProjectEdit::ReplaceController {
            id: id.clone(),
            value,
        }])
    }
    pub fn replace_sequence(&mut self, id: &SequenceId, value: Sequence) -> Result<(), String> {
        self.apply_edits([ProjectEdit::ReplaceSequence {
            id: id.clone(),
            value,
        }])
    }
}
