//! Resolve authoring identities once, before lowering anything for playback.

use crate::Retain;
use donder_model::Layout;
use donder_model::Patch;
use donder_model::SequenceId;
use donder_model::{AcceptedSequence, DonderProject};
use donder_model::{ControllerId, ControllerPort};
use indexmap::IndexSet;

pub(crate) struct SelectedPort<'a> {
    pub(crate) controller_index: u32,
    pub(crate) controller: &'a ControllerId,
    pub(crate) port: &'a ControllerPort,
}

/// What plays, a sequence or a rig, and the parts of the active setup it
/// plays on.
pub(crate) struct Selection<'a, S = AcceptedSequence<'a>> {
    pub(crate) show: S,
    pub(crate) layout: &'a Layout,
    pub(crate) geometry: &'a [(
        donder_model::FixtureInstanceId,
        donder_runtime_types::FixtureGeometry,
    )],
    pub(crate) patch: &'a Patch,
    pub(crate) encodings:
        &'a indexmap::IndexMap<donder_model::PixelRouteId, donder_runtime_types::OutputEncoding>,
    pub(crate) ports: Vec<SelectedPort<'a>>,
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

pub(crate) fn resolve_rig<'a>(
    project: &'a DonderProject,
    rig: &donder_model::RigId,
    retain: Retain<'_>,
) -> Option<Selection<'a, donder_model::AcceptedRig<'a>>> {
    let accepted = project.accepted_rig(rig)?;
    resolve_show(project, accepted, retain, |layout| {
        accepted
            .rig()
            .clips()
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
    let setup = project.get::<donder_model::Setup>(*project.root().setup.id())?;
    let layout = project.get::<donder_model::Layout>(*setup.layout.id())?;
    let geometry = project.playback_geometry(&layout.id)?;
    let patch = project.get::<donder_model::Patch>(*setup.patch.id())?;
    let encodings = project.playback_patch_encodings(&patch.id)?;
    if !targets_only(&layout.id) {
        return None;
    }
    let controllers = setup
        .controllers
        .iter()
        .enumerate()
        .map(|(index, source)| {
            // Project admission proves every setup controller index fits u32.
            project
                .get::<donder_model::Controller>(*source.id())
                .map(|controller| (index as u32, controller))
        })
        .collect::<Option<Vec<_>>>()?;
    let mut ports = Vec::new();
    match retain {
        Retain::All => {
            for (controller_index, controller) in controllers {
                ports.extend(controller.ports.iter().map(|port| SelectedPort {
                    controller_index,
                    controller: &controller.id,
                    port,
                }));
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
                ports.extend(controller.ports.iter().map(|port| SelectedPort {
                    controller_index,
                    controller: &controller.id,
                    port,
                }));
            }
        }
        Retain::Ports(requested) => {
            let mut seen = IndexSet::new();
            for (id, port_id) in requested {
                if !seen.insert((id, port_id)) {
                    continue;
                }
                let (controller_index, controller) = controllers
                    .iter()
                    .copied()
                    .find(|(_, controller)| &controller.id == id)?;
                let port = controller.ports.iter().find(|port| port.id == *port_id)?;
                ports.push(SelectedPort {
                    controller_index,
                    controller: &controller.id,
                    port,
                });
            }
        }
    }
    Some(Selection {
        show,
        layout,
        geometry,
        patch,
        encodings,
        ports,
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
            .project
    }

    fn addresses(selection: &Selection<'_>) -> Vec<(u32, u32)> {
        selection
            .ports
            .iter()
            .map(|port| (port.controller_index, port.port.id.0))
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
                all.ports
                    .iter()
                    .filter(move |port| port.controller_index == index)
                    .map(move |port| (index, port.port.id.0))
            })
            .collect::<Vec<_>>();
        assert_eq!(addresses(&selected), expected);
    }

    #[test]
    fn port_selection_is_a_stable_set_and_unknown_ports_reject_the_whole_selection() {
        let project = starter();
        let sequence = project.root().sequences[0].id();
        let all = resolve(&project, sequence, Retain::All).unwrap();
        let first = &all.ports[0];
        let last = all.ports.last().unwrap();
        let requested = [
            (*last.controller, last.port.id),
            (*first.controller, first.port.id),
            (*last.controller, last.port.id),
        ];
        let selected = resolve(&project, sequence, Retain::Ports(&requested)).unwrap();
        assert_eq!(
            addresses(&selected),
            [
                (last.controller_index, last.port.id.0),
                (first.controller_index, first.port.id.0),
            ]
        );
        let unknown = [(*first.controller, ControllerPortId(u32::MAX))];
        assert!(resolve(&project, sequence, Retain::Ports(&unknown)).is_none());
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
        for retain in [Retain::Controllers(&[]), Retain::Ports(&[])] {
            assert!(
                resolve(&project, &sequence, retain)
                    .unwrap()
                    .ports
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
