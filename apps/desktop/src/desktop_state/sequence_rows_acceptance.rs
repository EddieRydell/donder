use super::DesktopState;
use donder_editor::dto::{
    FixtureTarget, GuiDocument, GuiDocumentChange, GuiDocumentRequest, GuiEditCommand,
    GuiEditUpdate, LayoutGuiEdit, SequenceAutomationResize, SequenceAutomationTarget,
    SequenceClipRef, SequenceEffectReference, SequenceGuiEdit, SequencePasteAnchor,
    SequenceResizeEdge, SequenceSelection,
};
use donder_model::{AutomationTarget, Sequence, SequenceId};
use std::sync::Arc;

struct Timeline {
    state: DesktopState,
    id: SequenceId,
    effect: u32,
    automation: u32,
    root: camino::Utf8PathBuf,
    _temporary: tempfile::TempDir,
}

#[test]
fn moving_and_resizing_a_mixed_selection_is_atomic_and_never_rebinds_automation() {
    let timeline = Timeline::new();
    let before = timeline.sequence();
    timeline.selection_edit(SequenceGuiEdit::MoveClips {
        clips: clip_refs(vec![timeline.effect], vec![timeline.automation]),
        time_delta_seconds: 1.0,
        anchor_lane: 0,
        lane_delta: 1,
    });
    let moved = timeline.sequence();
    let clip = moved
        .clips
        .iter()
        .find(|clip| clip.id.0 == timeline.effect)
        .unwrap();
    let automation = moved
        .automation_clips
        .iter()
        .find(|clip| clip.id.0 == timeline.automation)
        .unwrap();
    assert_eq!(clip.target.fixture.0, timeline.targets()[1].fixture);
    assert_eq!(automation.row_target, clip.target);
    assert_eq!(
        automation.bindings,
        before
            .automation_clips
            .iter()
            .find(|clip| clip.id.0 == timeline.automation)
            .unwrap()
            .bindings
    );
    timeline.state.undo_active_edit();
    assert_eq!(timeline.sequence(), before);
    timeline.state.redo_active_edit();
    assert_eq!(timeline.sequence(), moved);
    timeline.selection_edit(SequenceGuiEdit::ResizeClips {
        clips: clip_refs(vec![timeline.effect], vec![timeline.automation]),
        edge: SequenceResizeEdge::Right,
        automation: SequenceAutomationResize::Crop,
        time_delta_seconds: 1.0,
    });
    let resized = timeline.sequence();
    assert_eq!(
        resized
            .clips
            .iter()
            .find(|clip| clip.id.0 == timeline.effect)
            .unwrap()
            .duration
            .as_seconds_f64(),
        clip.duration.as_seconds_f64() + 1.0
    );
    assert_eq!(
        resized
            .automation_clips
            .iter()
            .find(|clip| clip.id.0 == timeline.automation)
            .unwrap()
            .duration
            .as_seconds_f64(),
        automation.duration.as_seconds_f64() + 1.0
    );
    timeline.state.undo_active_edit();
    assert_eq!(timeline.sequence(), moved);
}

impl Timeline {
    fn new() -> Self {
        let (temporary, root) = crate::desktop_foundation_tests::tests::starter_copy();
        let path = root.join("effects/mark-impact-burst.donder");
        let mut source = std::fs::read_to_string(&path).unwrap();
        source.push_str("\neffect RowTest { param level: float in 0.0..1.0 = 0.5; sample { rgb(level, level, level) } }\n");
        std::fs::write(path, source).unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        let mut settings = state.snapshot().settings;
        settings.autosave_project_edits = false;
        state.update_app_settings(settings);
        let session = state.project_session().unwrap();
        let sequence = session
            .project
            .reusable::<donder_model::Sequence>()
            .values()
            .find(|sequence| !sequence.clips.is_empty())
            .unwrap();
        let mut timeline = Self {
            state,
            id: sequence.id,
            effect: sequence.clips[0].id.0,
            automation: 0,
            root,
            _temporary: temporary,
        };
        timeline.edit(SequenceGuiEdit::ChangeEffectDefinition {
            id: timeline.effect,
            effect: SequenceEffectReference {
                path: "effects/mark-impact-burst.donder".into(),
                effect_name: "RowTest".into(),
            },
            initial_color: sequence.layers[0].color.to_hex(),
        });
        timeline.edit(SequenceGuiEdit::CreateAndBindAutomationClip {
            target: SequenceAutomationTarget::ClipParam {
                clip_id: timeline.effect,
                param: "level".into(),
            },
        });
        timeline.automation = timeline.sequence().automation_clips.iter().find(|clip| clip.bindings.iter().any(|binding|
            matches!(&binding.target, AutomationTarget::ClipParam { clip_id, .. } if clip_id.0 == timeline.effect)
        )).unwrap().id.0;
        timeline
    }

    fn request(&self) -> GuiDocumentRequest {
        let address = self
            .state
            .project_session()
            .unwrap()
            .address_of(self.id.into())
            .unwrap();
        GuiDocumentRequest {
            project_revision: self.state.snapshot().project_revision,
            path: address.document().to_string(),
            object_key: address.root_source().object().into(),
            owned_path: Vec::new(),
        }
    }

    fn edit(&self, edit: SequenceGuiEdit) {
        let result = self
            .state
            .apply_gui_edit(self.request(), GuiEditCommand::Sequence { edit });
        assert!(
            matches!(result.document, GuiDocument::Sequence { .. }),
            "{:?}",
            result.document
        );
    }

    fn selection_edit(&self, edit: SequenceGuiEdit) -> GuiEditUpdate {
        let result = self
            .state
            .edit_gui_document(self.request(), GuiEditCommand::Sequence { edit });
        assert!(
            matches!(
                result.change,
                GuiDocumentChange::SequenceClips { .. }
                    | GuiDocumentChange::Document {
                        document: GuiDocument::Sequence { .. }
                    }
            ),
            "{:?}",
            result.change
        );
        result
    }

    fn copy(&self, selection: SequenceSelection) {
        self.state
            .copy_sequence_selection(self.request(), selection);
    }

    fn sequence(&self) -> Sequence {
        self.state
            .project_session()
            .unwrap()
            .project
            .get::<donder_model::Sequence>(self.id)
            .unwrap()
            .clone()
    }

    fn selection(&self) -> SequenceSelection {
        clips(vec![self.effect], vec![self.automation])
    }

    /// The target of every timeline lane, in display order.
    fn targets(&self) -> Vec<FixtureTarget> {
        let GuiDocument::Sequence { document } =
            self.state.get_gui_document(self.request()).document
        else {
            panic!("sequence document missing")
        };
        document.lanes.into_iter().map(|lane| lane.target).collect()
    }

    fn paste(&self, lane: u32) -> SequenceSelection {
        self.selection_edit(SequenceGuiEdit::Paste {
            anchor: SequencePasteAnchor {
                lane: Some(lane),
                time_seconds: 0.0,
            },
        })
        .selection
        .unwrap()
    }
}

#[test]
fn mixed_clipboard_remaps_bindings_and_is_one_undoable_persisted_edit() {
    let timeline = Timeline::new();
    let before = timeline.sequence();
    timeline.copy(timeline.selection());
    let target = timeline.targets()[1].clone();
    let (clip_ids, automation_ids) = ids(timeline.paste(1));
    let pasted = timeline.sequence();
    let clip = pasted
        .clips
        .iter()
        .find(|clip| clip.id.0 == clip_ids[0])
        .unwrap();
    let automation = pasted
        .automation_clips
        .iter()
        .find(|clip| clip.id.0 == automation_ids[0])
        .unwrap();
    assert_eq!(clip.target.fixture.0, target.fixture);
    assert_eq!(automation.row_target, clip.target);
    assert!(
        matches!(&automation.bindings[0].target, AutomationTarget::ClipParam { clip_id, .. } if clip_id == &clip.id)
    );
    assert_ne!(clip.id.0, timeline.effect);
    assert_ne!(automation.id.0, timeline.automation);
    timeline.state.undo_active_edit();
    assert_eq!(timeline.sequence(), before);
    timeline.state.redo_active_edit();
    assert_eq!(timeline.sequence(), pasted);
    timeline.state.save_all().unwrap();
    let reopened = donder_project_io::check_project(
        &timeline.root,
        &Default::default(),
        &mut Default::default(),
    )
    .into_session()
    .unwrap();
    assert_eq!(
        reopened
            .project
            .get::<donder_model::Sequence>(timeline.id)
            .unwrap(),
        &pasted
    );
}

#[test]
fn out_of_bounds_paste_is_atomic_and_does_not_consume_history() {
    let timeline = Timeline::new();
    timeline.copy(timeline.selection());
    let before = timeline.sequence();
    let (clip_ids, _) = ids(timeline.paste(1));
    timeline.copy(clips(vec![timeline.effect, clip_ids[0]], vec![]));
    let session = timeline.state.project_session().unwrap();
    let rejected = timeline.state.edit_gui_document(
        timeline.request(),
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::Paste {
                anchor: SequencePasteAnchor {
                    lane: Some(timeline.targets().len() as u32 - 1),
                    time_seconds: 0.0,
                },
            },
        },
    );
    assert!(matches!(
        rejected.change,
        GuiDocumentChange::Document {
            document: GuiDocument::Blocked { .. }
        }
    ));
    assert!(Arc::ptr_eq(
        &session,
        &timeline.state.project_session().unwrap()
    ));
    timeline.state.undo_active_edit();
    assert_eq!(timeline.sequence(), before);
}

#[test]
fn copying_an_envelope_is_unbound_but_cutting_preserves_available_bindings() {
    let timeline = Timeline::new();
    let selection = || clips(vec![], vec![timeline.automation]);
    timeline.copy(selection());
    let (_, automation_ids) = ids(timeline.paste(1));
    let sequence = timeline.sequence();
    assert!(
        sequence
            .automation_clips
            .iter()
            .find(|clip| clip.id.0 == automation_ids[0])
            .unwrap()
            .bindings
            .is_empty()
    );
    let original = sequence
        .automation_clips
        .iter()
        .find(|clip| clip.id.0 == timeline.automation)
        .unwrap();
    timeline.selection_edit(SequenceGuiEdit::CutSelection {
        selection: selection(),
    });
    let (_, automation_ids) = ids(timeline.paste(2));
    let pasted = timeline.sequence();
    let moved = pasted
        .automation_clips
        .iter()
        .find(|clip| clip.id.0 == automation_ids[0])
        .unwrap();
    assert_eq!(moved.bindings, original.bindings);
    assert_eq!(moved.curve, original.curve);
}

#[test]
fn shared_members_have_a_lane_per_group_and_move_from_the_nearest_copy() {
    let timeline = Timeline::new();
    let session = timeline.state.project_session().unwrap();
    let setup = session
        .project
        .get::<donder_model::Setup>(*session.project.root().setup.id())
        .unwrap();
    let layout = session.address_of((*setup.layout.id()).into()).unwrap();
    let request = GuiDocumentRequest {
        project_revision: timeline.state.snapshot().project_revision,
        path: layout.document().to_string(),
        object_key: layout.root_source().object().into(),
        owned_path: Vec::new(),
    };
    let GuiDocument::Layout { document: layout } =
        timeline.state.get_gui_document(request.clone()).document
    else {
        panic!("layout document missing")
    };
    let lanes = timeline.targets();
    let (first, second) = (lanes[1].fixture, lanes[2].fixture);
    let mut fixtures = layout.fixtures;
    fixtures.push(donder_editor::dto::GuiLayoutFixture {
        id: 100,
        name: "Pair".into(),
        description: None,
        kind: donder_editor::dto::GuiLayoutFixtureKind::Group {
            members: vec![first, second],
        },
    });
    let mut root = layout.root;
    root.push(100);
    let result = timeline.state.apply_gui_edit(
        GuiDocumentRequest {
            project_revision: timeline.state.snapshot().project_revision,
            ..request
        },
        GuiEditCommand::Layout {
            edit: LayoutGuiEdit::SetFixtures { fixtures, root },
        },
    );
    assert!(
        matches!(result.document, GuiDocument::Layout { .. }),
        "{:?}",
        result.document
    );
    // The shared fixtures appear under both groups, as the same targets.
    let GuiDocument::Sequence { document } =
        timeline.state.get_gui_document(timeline.request()).document
    else {
        panic!("sequence document missing")
    };
    let pair = lanes.len();
    let lanes = document.lanes;
    assert_eq!(lanes.len(), pair + 3);
    assert_eq!(lanes[pair].target.fixture, 100);
    assert_eq!(lanes[pair + 1].target.fixture, first);
    assert_eq!(lanes[pair + 2].target.fixture, second);
    assert_eq!((lanes[1].occurrences, lanes[pair + 1].occurrences), (2, 2));
    assert_eq!((lanes[0].occurrences, lanes[pair].occurrences), (1, 1));
    assert_eq!(lanes[pair + 1].depth, 1);
    let clip_target = || {
        timeline
            .sequence()
            .clips
            .iter()
            .find(|clip| clip.id.0 == timeline.effect)
            .unwrap()
            .target
            .fixture
            .0
    };
    let move_clip = |anchor_lane: usize, lane_delta| {
        timeline.selection_edit(SequenceGuiEdit::MoveClips {
            clips: clip_refs(vec![timeline.effect], vec![]),
            time_delta_seconds: 0.0,
            anchor_lane: anchor_lane as u32,
            lane_delta,
        });
    };
    move_clip(0, 1);
    assert_eq!(clip_target(), first);
    // Moving up from the copy under `Pair` reaches `Pair`, not the lane above
    // the first copy.
    move_clip(pair + 1, -1);
    assert_eq!(clip_target(), 100);
}

/// A selection of clips and automation clips.
fn clips(effects: Vec<u32>, automation: Vec<u32>) -> SequenceSelection {
    SequenceSelection::Clips {
        clips: clip_refs(effects, automation),
    }
}

fn clip_refs(effects: Vec<u32>, automation: Vec<u32>) -> Vec<SequenceClipRef> {
    effects
        .into_iter()
        .map(|id| SequenceClipRef::Clip { id })
        .chain(
            automation
                .into_iter()
                .map(|id| SequenceClipRef::Automation { id }),
        )
        .collect()
}

/// The clip and automation clip ids of a clip selection.
fn ids(selection: SequenceSelection) -> (Vec<u32>, Vec<u32>) {
    let SequenceSelection::Clips { clips } = selection else {
        panic!("clip selection missing")
    };
    let mut clip_ids = Vec::new();
    let mut automation = Vec::new();
    for clip in clips {
        match clip {
            SequenceClipRef::Clip { id } => clip_ids.push(id),
            SequenceClipRef::Automation { id } => automation.push(id),
        }
    }
    (clip_ids, automation)
}
