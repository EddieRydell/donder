//! Resolve authoring identities once, before lowering anything for playback.

use crate::Retain;
use donder_model::Layout;
use donder_model::PixelRoute;
use donder_model::{AcceptedShow, DonderProject, ShowId};
use donder_model::{ControllerId, ControllerOutput};
use indexmap::IndexSet;

/// One output record of the prepared show, and its width in bytes.
pub(crate) struct SelectedOutput<'a> {
    pub(crate) controller_index: u32,
    pub(crate) controller: &'a ControllerId,
    pub(crate) output: ControllerOutput,
    pub(crate) width: u32,
}

/// What plays, a sequence or a console, and the parts of the project it
/// plays on.
pub(crate) struct Selection<'a> {
    pub(crate) show: AcceptedShow<'a>,
    pub(crate) layout: &'a Layout,
    pub(crate) geometry: &'a [(
        donder_model::FixtureId,
        donder_runtime_types::FixtureGeometry,
    )],
    pub(crate) pixel_routes: &'a [PixelRoute],
    pub(crate) routes:
        &'a indexmap::IndexMap<donder_model::PixelRouteId, donder_model::PlaybackRoute>,
    pub(crate) outputs: Vec<SelectedOutput<'a>>,
}

/// `show` on the project's layout and controllers.
pub(crate) fn resolve<'a>(
    project: &'a DonderProject,
    show: ShowId,
    retain: Retain<'_>,
) -> Option<Selection<'a>> {
    let show = project.accepted(show)?;
    let layout = project.layout();
    // Project admission proves every controller index fits u32.
    let controllers = project
        .reusable::<donder_model::Controller>()
        .iter()
        .enumerate()
        .map(|(index, (id, controller))| (index as u32, id, controller))
        .collect::<Vec<_>>();
    let mut outputs = Vec::new();
    let all = |controller_index,
               id: &'a donder_model::ControllerId,
               controller: &'a donder_model::Controller| {
        controller
            .outputs()
            .into_iter()
            .map(move |(output, width)| SelectedOutput {
                controller_index,
                controller: id,
                output,
                width,
            })
    };
    match retain {
        Retain::All => {
            for (controller_index, id, controller) in controllers {
                outputs.extend(all(controller_index, id, controller));
            }
        }
        Retain::Controllers(requested) => {
            let mut seen = IndexSet::new();
            for id in requested {
                if !seen.insert(id) {
                    continue;
                }
                let (controller_index, id, controller) = controllers
                    .iter()
                    .copied()
                    .find(|(_, candidate, _)| *candidate == id)?;
                outputs.extend(all(controller_index, id, controller));
            }
        }
        Retain::Outputs(requested) => {
            let mut seen = IndexSet::new();
            for (id, output) in requested {
                if !seen.insert((id, output)) {
                    continue;
                }
                let (controller_index, id, controller) = controllers
                    .iter()
                    .copied()
                    .find(|(_, candidate, _)| *candidate == id)?;
                outputs.push(SelectedOutput {
                    controller_index,
                    controller: id,
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
        pixel_routes: &project.root().routes,
        routes: project.playback_routes(),
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
        let sequence = project
            .reusable::<donder_model::Sequence>()
            .get_index(0)
            .unwrap()
            .0;
        let controllers_store = project.reusable::<donder_model::Controller>();
        let mut controllers = controllers_store.keys().rev().copied().collect::<Vec<_>>();
        controllers.push(controllers[0]);
        let selected = resolve(
            &project,
            ShowId::Sequence(*sequence),
            Retain::Controllers(&controllers),
        )
        .unwrap();
        let all = resolve(&project, ShowId::Sequence(*sequence), Retain::All).unwrap();
        let expected = (0..controllers_store.len())
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
        let sequence = project
            .reusable::<donder_model::Sequence>()
            .get_index(0)
            .unwrap()
            .0;
        let all = resolve(&project, ShowId::Sequence(*sequence), Retain::All).unwrap();
        let first = &all.outputs[0];
        let last = all.outputs.last().unwrap();
        let requested = [
            (*last.controller, last.output),
            (*first.controller, first.output),
            (*last.controller, last.output),
        ];
        let selected = resolve(
            &project,
            ShowId::Sequence(*sequence),
            Retain::Outputs(&requested),
        )
        .unwrap();
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
        assert!(
            resolve(
                &project,
                ShowId::Sequence(*sequence),
                Retain::Outputs(&unknown)
            )
            .is_none()
        );
    }

    #[test]
    fn an_unknown_controller_is_not_silently_omitted() {
        let project = starter();
        let sequence = project
            .reusable::<donder_model::Sequence>()
            .get_index(0)
            .unwrap()
            .0;
        let controllers_store = project.reusable::<donder_model::Controller>();
        let first = controllers_store.get_index(0).unwrap().0;
        let missing = ControllerId(u32::MAX);
        assert!(
            resolve(
                &project,
                ShowId::Sequence(*sequence),
                Retain::Controllers(&[*first, missing])
            )
            .is_none()
        );
    }

    #[test]
    fn empty_output_lists_are_valid_but_absent_sequences_are_not_selected() {
        let mut project = starter();
        let sequence = *project
            .reusable::<donder_model::Sequence>()
            .get_index(0)
            .unwrap()
            .0;
        for retain in [Retain::Controllers(&[]), Retain::Outputs(&[])] {
            assert!(
                resolve(&project, ShowId::Sequence(sequence), retain)
                    .unwrap()
                    .outputs
                    .is_empty()
            );
        }
        let edits = project
            .reusable::<donder_model::Sequence>()
            .keys()
            .cloned()
            .map(|id| donder_model::ProjectEdit::Remove(id.into()))
            .collect::<Vec<_>>();
        project.apply_edits(edits).unwrap();
        assert!(resolve(&project, ShowId::Sequence(sequence), Retain::All).is_none());
    }
}
