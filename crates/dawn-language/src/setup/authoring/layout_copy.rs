use crate::element::{ElementNodeKind, ElementTreeId};
use crate::fixture_profile::FixtureProfileId;
use crate::model::DawnProject;
use crate::patch::{PatchId, PatchNode};
use crate::preview::{PreviewLayoutId, PropDefinitionId};
use crate::sequence::SequenceId;
use crate::setup::SetupId;
use indexmap::IndexMap;

pub struct SetupLayoutCopy {
    pub elements: ElementTreeId,
    pub preview: PreviewLayoutId,
    pub patch: PatchId,
    pub props: IndexMap<PropDefinitionId, PropDefinitionId>,
    pub profiles: IndexMap<FixtureProfileId, FixtureProfileId>,
}

/// Copy layout objects while preserving numeric element, placement, and patch IDs.
/// Callers select the sequences to retarget and own source registration and edits.
pub fn copy_setup_layout(
    project: &mut DawnProject,
    setup_id: &SetupId,
    copy: SetupLayoutCopy,
    sequences: &[SequenceId],
) -> Result<(), String> {
    let setup = project.setups.get(setup_id).ok_or("Setup was not found.")?;
    let original_tree = setup.elements.clone();
    if project.element_trees.contains_key(&copy.elements)
        || project.preview_layouts.contains_key(&copy.preview)
        || project.patches.contains_key(&copy.patch)
    {
        return Err("Editable layout copies require new source identities.".into());
    }
    if sequences
        .iter()
        .any(|id| !project.sequences.contains_key(id))
    {
        return Err("A sequence selected for layout copying is missing.".into());
    }
    let mut tree = project
        .element_trees
        .get(&setup.elements)
        .ok_or("Element tree was not found.")?
        .clone();
    let mut preview = project
        .preview_layouts
        .get(&setup.preview)
        .ok_or("Preview layout was not found.")?
        .clone();
    let mut patch = project
        .patches
        .get(&setup.patch)
        .ok_or("Patch was not found.")?
        .clone();
    tree.id = copy.elements.clone();
    preview.id = copy.preview.clone();
    preview.element_tree = copy.elements.clone();
    patch.id = copy.patch.clone();
    for (original, id) in &copy.props {
        if project.definitions.props.definitions.contains_key(id) {
            return Err("Copied prop definition already exists.".into());
        }
        let definition = project
            .definitions
            .props
            .definitions
            .get(original)
            .ok_or("Prop definition was not found.")?
            .clone();
        project
            .definitions
            .props
            .definitions
            .insert(id.clone(), definition);
    }
    for prop in &mut preview.props {
        prop.definition = copy
            .props
            .get(&prop.definition)
            .ok_or("A preview shape was not selected for copying.")?
            .clone();
    }
    for (original, id) in &copy.profiles {
        if project
            .definitions
            .fixture_profiles
            .definitions
            .contains_key(id)
        {
            return Err("Copied fixture profile already exists.".into());
        }
        let mut profile = project
            .definitions
            .fixture_profiles
            .definitions
            .get(original)
            .ok_or("Fixture profile was not found.")?
            .clone();
        profile.id = id.clone();
        project
            .definitions
            .fixture_profiles
            .definitions
            .insert(id.clone(), profile);
    }
    for node in tree.nodes.values_mut() {
        if let ElementNodeKind::Fixture { profile } = &mut node.kind {
            *profile = copy
                .profiles
                .get(profile)
                .ok_or("A fixture profile was not selected for copying.")?
                .clone();
        }
    }
    for node in patch.nodes.values_mut() {
        if let Some(profile) = node.fixture_profile_mut() {
            *profile = copy
                .profiles
                .get(profile)
                .ok_or("A patch fixture profile was not selected for copying.")?
                .clone();
        }
        if let PatchNode::Source(source) = node
            && source.selection.tree == original_tree
        {
            source.selection.tree = copy.elements.clone();
        }
    }
    for id in sequences {
        let sequence = project
            .sequences
            .get_mut(id)
            .ok_or("Sequence was not found.")?;
        for selection in sequence
            .effects
            .iter_mut()
            .map(|effect| &mut effect.target)
            .chain(
                sequence
                    .control_clips
                    .iter_mut()
                    .map(|clip| clip.target.selection_mut()),
            )
        {
            if selection.tree == original_tree {
                selection.tree = copy.elements.clone();
            }
        }
    }
    project.element_trees.insert(copy.elements.clone(), tree);
    project
        .preview_layouts
        .insert(copy.preview.clone(), preview);
    project.patches.insert(copy.patch.clone(), patch);
    let setup = project
        .setups
        .get_mut(setup_id)
        .ok_or("Setup was not found.")?;
    setup.elements = copy.elements;
    setup.preview = copy.preview;
    setup.patch = copy.patch;
    Ok(())
}
