use crate::layout::LayoutId;
use crate::model::DonderProject;
use crate::patch::PatchId;
use crate::sequence::SequenceId;
use crate::setup::SetupId;

/// Copy layout instances and routing, retaining their shared fixture definitions.
/// The caller owns transactionality and source registration.
pub fn copy_setup_layout(
    project: &mut DonderProject,
    setup_id: &SetupId,
    layout_id: LayoutId,
    patch_id: PatchId,
    sequences: &[SequenceId],
) -> Result<(), String> {
    let setup = project.setups.get(setup_id).ok_or("Setup was not found.")?;
    if project.layouts.contains_key(&layout_id) || project.patches.contains_key(&patch_id) {
        return Err("Editable copies require new source identities.".into());
    }
    let original = setup.layout.clone();
    let mut layout = project
        .layouts
        .get(&original)
        .ok_or("Layout was not found.")?
        .clone();
    let mut patch = project
        .patches
        .get(&setup.patch)
        .ok_or("Patch was not found.")?
        .clone();
    layout.id = layout_id.clone();
    patch.id = patch_id.clone();
    for route in &mut patch.routes {
        if route.target.layout == original {
            route.target.layout = layout_id.clone();
        }
    }
    for id in sequences {
        let sequence = project
            .sequences
            .get_mut(id)
            .ok_or("Sequence was not found.")?;
        for effect in &mut sequence.effects {
            if effect.target.layout == original {
                effect.target.layout = layout_id.clone();
            }
        }
    }
    project.layouts.insert(layout_id.clone(), layout);
    project.patches.insert(patch_id.clone(), patch);
    let setup = project
        .setups
        .get_mut(setup_id)
        .ok_or("Setup was not found.")?;
    setup.layout = layout_id;
    setup.patch = patch_id;
    Ok(())
}
