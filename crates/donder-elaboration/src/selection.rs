//! Resolve authoring identities once, before lowering anything for playback.

use crate::Retain;
use donder_model::Layout;
use donder_model::Patch;
use donder_model::SequenceId;
use donder_model::{AcceptedSequence, DonderProject};
use donder_model::{ControllerId, ControllerOutput};
use indexmap::IndexSet;

/// One output record of the prepared show, and its width in bytes.
pub(crate) struct SelectedOutput<'a> {
    pub(crate) controller_index: u32,
    pub(crate) controller: &'a ControllerId,
    pub(crate) output: ControllerOutput,
    pub(crate) width: u32,
}

/// What plays, a sequence or a console, and the parts of the active setup it
/// plays on.
pub(crate) struct Selection<'a, S = AcceptedSequence<'a>> {
    pub(crate) show: S,
    pub(crate) layout: &'a Layout,
    pub(crate) geometry: &'a [(
        donder_model::FixtureInstanceId,
        donder_runtime_types::FixtureGeometry,
    )],
    pub(crate) patch: &'a Patch,
    pub(crate) routes:
        &'a indexmap::IndexMap<donder_model::PixelRouteId, donder_model::PlaybackRoute>,
    pub(crate) outputs: Vec<SelectedOutput<'a>>,
}

pub(crate) fn resolve<'a>(
    project: &'a DonderProject,
    sequence: &SequenceId,
    retain: Retain<'_>,
) -> Option<Selection<'a>> {
    let accepted = project.accepted_sequence(sequence)?;
    let sequence = accepted.sequence();
    // Reusable sequences may belong to another layout. They remain authorable,
    // but cannot be played against this project's active setup.
    resolve_show(project, accepted, retain, |layout| {
        sequence
            .clips
            .iter()
            .all(|clip| &clip.target.layout == layout)
            && sequence
                .automation_clips
                .iter()
                .all(|clip| &clip.row_target.layout == layout)
    })
}

pub(crate) fn resolve_console<'a>(
    project: &'a DonderProject,
    console: &donder_model::ConsoleId,
    retain: Retain<'_>,
) -> Option<Selection<'a, donder_model::AcceptedConsole<'a>>> {
    let accepted = project.accepted_console(console)?;
    resolve_show(project, accepted, retain, |layout| {
        accepted
            .console()
            .clips
            .iter()
            .all(|clip| &clip.target.layout == layout)
    })
}

/// `show` on the project's active setup, when it targets only the setup's
/// layout.
fn resolve_show<'a, S>(
    project: &'a DonderProject,
    show: S,
    retain: Retain<'_>,
    targets_only: impl FnOnce(&donder_model::LayoutId) -> bool,
) -> Option<Selection<'a, S>> {
    let setup = project.active_setup();
    let layout = setup.layout();
    if !targets_only(&layout.id) {
        return None;
    }
    let patch = setup.patch();
    // Project admission proves every setup controller index fits u32.
    let controllers = setup
        .controllers()
        .enumerate()
        .map(|(index, controller)| (index as u32, controller.object()))
        .collect::<Vec<_>>();
    let mut outputs = Vec::new();
    let all = |controller_index, controller: &'a donder_model::Controller| {
        controller
            .outputs()
            .into_iter()
            .map(move |(output, width)| SelectedOutput {
                controller_index,
                controller: &controller.id,
                output,
                width,
            })
    };
    match retain {
        Retain::All => {
            for (controller_index, controller) in controllers {
                outputs.extend(all(controller_index, controller));
            }
        }
        Retain::Controllers(requested) => {
            let mut seen = IndexSet::new();
            for id in requested {
                if !seen.insert(id) {
                    continue;
                }
                let (controller_index, controller) = controllers
                    .iter()
                    .copied()
                    .find(|(_, controller)| &controller.id == id)?;
                outputs.extend(all(controller_index, controller));
            }
        }
        Retain::Outputs(requested) => {
            let mut seen = IndexSet::new();
            for (id, output) in requested {
                if !seen.insert((id, output)) {
                    continue;
                }
                let (controller_index, controller) = controllers
                    .iter()
                    .copied()
                    .find(|(_, controller)| &controller.id == id)?;
                outputs.push(SelectedOutput {
                    controller_index,
                    controller: &controller.id,
                    output: *output,
                    width: controller.output_width(*output)?,
                });
            }
        }
    }
    Some(Selection {
        show,
        layout: layout.object(),
        geometry: layout.playback_geometry(),
        patch: patch.object(),
        routes: patch.playback_routes(),
        outputs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use donder_model::ControllerPortId;

    fn starter() -> DonderProject {
        let path = camino::Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
        donder_project_io::check_project(&path, &Default::default(), &mut Default::default())
            .into_session()
            .unwrap()
            .project()
            .clone()
    }

    fn addresses(selection: &Selection<'_>) -> Vec<(u32, ControllerOutput)> {
        selection
            .outputs
            .iter()
            .map(|output| (output.controller_index, output.output))
            .collect()
    }

    #[test]
    fn controller_selection_preserves_requested_order_and_original_indices() {
        let project = starter();
        let sequence = project.root().sequences[0].id();
        let setup = project
            .get::<donder_model::Setup>(*project.root().setup.id())
            .unwrap();
        let mut controllers = setup
            .controllers
            .iter()
            .rev()
            .map(|source| *source.id())
            .collect::<Vec<_>>();
        controllers.push(controllers[0]);
        let selected = resolve(&project, sequence, Retain::Controllers(&controllers)).unwrap();
        let all = resolve(&project, sequence, Retain::All).unwrap();
        let expected = (0..setup.controllers.len())
            .rev()
            .map(|index| index as u32)
            .flat_map(|index| {
                all.outputs
                    .iter()
                    .filter(move |output| output.controller_index == index)
                    .map(move |output| (index, output.output))
            })
            .collect::<Vec<_>>();
        assert_eq!(addresses(&selected), expected);
    }

    #[test]
    fn port_selection_is_a_stable_set_and_unknown_ports_reject_the_whole_selection() {
        let project = starter();
        let sequence = project.root().sequences[0].id();
        let all = resolve(&project, sequence, Retain::All).unwrap();
        let first = &all.outputs[0];
        let last = all.outputs.last().unwrap();
        let requested = [
            (*last.controller, last.output),
            (*first.controller, first.output),
            (*last.controller, last.output),
        ];
        let selected = resolve(&project, sequence, Retain::Outputs(&requested)).unwrap();
        let expected = [
            (last.controller_index, last.output),
            (first.controller_index, first.output),
        ]
        .into_iter()
        .collect::<IndexSet<_>>();
        assert_eq!(
            addresses(&selected),
            expected.into_iter().collect::<Vec<_>>()
        );
        let unknown = [(
            *first.controller,
            ControllerOutput::Port(ControllerPortId(u32::MAX)),
        )];
        assert!(resolve(&project, sequence, Retain::Outputs(&unknown)).is_none());
    }

    #[test]
    fn an_unknown_controller_is_not_silently_omitted() {
        let project = starter();
        let sequence = project.root().sequences[0].id();
        let setup = project
            .get::<donder_model::Setup>(*project.root().setup.id())
            .unwrap();
        let first = setup.controllers[0].id();
        let missing = ControllerId(u32::MAX);
        assert!(resolve(&project, sequence, Retain::Controllers(&[*first, missing])).is_none());
    }

    #[test]
    fn empty_output_lists_are_valid_but_absent_sequences_are_not_selected() {
        let mut project = starter();
        let sequence = *project.root().sequences[0].id();
        for retain in [Retain::Controllers(&[]), Retain::Outputs(&[])] {
            assert!(
                resolve(&project, &sequence, retain)
                    .unwrap()
                    .outputs
                    .is_empty()
            );
        }
        let mut root = project.root().clone();
        root.sequences.clear();
        let mut edits = project
            .reusable::<donder_model::Sequence>()
            .keys()
            .cloned()
            .map(|id| donder_model::ProjectEdit::Remove(id.into()))
            .collect::<Vec<_>>();
        edits.push(donder_model::ProjectEdit::Put(
            donder_model::Object::Project(root),
        ));
        project.apply_edits(edits).unwrap();
        assert!(resolve(&project, &sequence, Retain::All).is_none());
    }
}
