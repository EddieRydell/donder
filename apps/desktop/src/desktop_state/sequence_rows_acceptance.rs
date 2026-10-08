use super::DesktopState;
use crate::dto::{
    DocumentViewId, FixtureTarget, GuiDocument, GuiDocumentRequest, GuiEditCommand, LayoutGuiEdit,
    SequenceAutomationResize, SequenceAutomationTarget, SequenceEffectReference, SequenceGuiEdit,
    SequencePasteAnchor, SequenceResizeEdge, SequenceSelection, SequenceSelectionEdit,
    SequenceSelectionEditResult,
};
use donder_language::sequence::{AutomationTarget, Sequence, SequenceId};
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
    timeline.selection_edit(SequenceSelectionEdit::MoveClips {
        effect_ids: vec![timeline.effect],
        automation_ids: vec![timeline.automation],
        time_delta_seconds: 1.0,
        anchor_lane: 0,
        lane_delta: 1,
    });
    let moved = timeline.sequence();
    let effect = moved
        .effects
        .iter()
        .find(|effect| effect.id.0 == timeline.effect)
        .unwrap();
    let automation = moved
        .automation_clips
        .iter()
        .find(|clip| clip.id.0 == timeline.automation)
        .unwrap();
    assert_eq!(effect.target.fixture.0, timeline.targets()[1].fixture);
    assert_eq!(automation.row_target, effect.target);
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
    timeline.selection_edit(SequenceSelectionEdit::ResizeClips {
        effect_ids: vec![timeline.effect],
        automation_ids: vec![timeline.automation],
        edge: SequenceResizeEdge::Right,
        automation: SequenceAutomationResize::Crop,
        time_delta_seconds: 1.0,
    });
    let resized = timeline.sequence();
    assert_eq!(
        resized
            .effects
            .iter()
            .find(|clip| clip.id.0 == timeline.effect)
            .unwrap()
            .duration
            .as_seconds_f32(),
        effect.duration.as_seconds_f32() + 1.0
    );
    assert_eq!(
        resized
            .automation_clips
            .iter()
            .find(|clip| clip.id.0 == timeline.automation)
            .unwrap()
            .duration
            .as_seconds_f32(),
        automation.duration.as_seconds_f32() + 1.0
    );
    timeline.state.undo_active_edit();
    assert_eq!(timeline.sequence(), moved);
}

#[test]
fn missing_automation_row_target_is_rejected_before_history_or_projection_changes() {
    let timeline = Timeline::new();
    let before = timeline.state.project_session().unwrap();
    let rejected = timeline.state.apply_gui_edit(
        timeline.request(),
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::MoveAutomationClip {
                id: timeline.automation,
                start_seconds: 0.0,
                row_target: FixtureTarget { fixture: u32::MAX },
            },
        },
    );
    assert!(
        matches!(rejected.document, GuiDocument::Blocked { reason, .. } if reason.contains("row target"))
    );
    assert!(Arc::ptr_eq(
        &before,
        &timeline.state.project_session().unwrap()
    ));
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
            .reusable_sequences()
            .values()
            .find(|sequence| !sequence.effects.is_empty())
            .unwrap();
        let mut timeline = Self {
            state,
            id: sequence.id.clone(),
            effect: sequence.effects[0].id.0,
            automation: 0,
            root,
            _temporary: temporary,
        };
        timeline.edit(SequenceGuiEdit::ChangeEffectDefinition {
            id: timeline.effect,
            effect: SequenceEffectReference::Custom {
                module_id: timeline.id.0.module_id().to_string(),
                path: "effects/mark-impact-burst.donder".into(),
                effect_name: "RowTest".into(),
            },
            initial_color: sequence.layers[0].color.to_hex(),
        });
        timeline.edit(SequenceGuiEdit::CreateAndBindAutomationClip {
            target: SequenceAutomationTarget::EffectParam {
                effect_id: timeline.effect,
                param: "level".into(),
            },
        });
        timeline.automation = timeline.sequence().automation_clips.iter().find(|clip| clip.bindings.iter().any(|binding|
            matches!(&binding.target, AutomationTarget::EffectParam { effect_id, .. } if effect_id.0 == timeline.effect)
        )).unwrap().id.0;
        timeline
    }

    fn request(&self) -> GuiDocumentRequest {
        GuiDocumentRequest {
            project_revision: self.state.snapshot().project_revision,
            path: self.id.0.document().to_string(),
            view: DocumentViewId::Sequence,
            object_key: Some(self.id.0.root_source().object().into()),
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

    fn selection_edit(&self, edit: SequenceSelectionEdit) -> SequenceSelectionEditResult {
        let result = self
            .state
            .apply_sequence_selection_edit(self.request(), edit);
        assert!(
            matches!(result.document, GuiDocument::Sequence { .. }),
            "{:?}",
            result.document
        );
        result
    }

    fn sequence(&self) -> Sequence {
        self.state
            .project_session()
            .unwrap()
            .project
            .sequence(&self.id)
            .unwrap()
            .clone()
    }

    fn selection(&self) -> SequenceSelection {
        SequenceSelection::Clips {
            effect_ids: vec![self.effect],
            automation_ids: vec![self.automation],
        }
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
        self.selection_edit(SequenceSelectionEdit::Paste {
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
    timeline.selection_edit(SequenceSelectionEdit::Copy {
        selection: timeline.selection(),
    });
    let target = timeline.targets()[1].clone();
    let SequenceSelection::Clips {
        effect_ids,
        automation_ids,
    } = timeline.paste(1)
    else {
        panic!("clip selection missing")
    };
    let pasted = timeline.sequence();
    let effect = pasted
        .effects
        .iter()
        .find(|effect| effect.id.0 == effect_ids[0])
        .unwrap();
    let automation = pasted
        .automation_clips
        .iter()
        .find(|clip| clip.id.0 == automation_ids[0])
        .unwrap();
    assert_eq!(effect.target.fixture.0, target.fixture);
    assert_eq!(automation.row_target, effect.target);
    assert!(
        matches!(&automation.bindings[0].target, AutomationTarget::EffectParam { effect_id, .. } if effect_id == &effect.id)
    );
    assert_ne!(effect.id.0, timeline.effect);
    assert_ne!(automation.id.0, timeline.automation);
    timeline.state.undo_active_edit();
    assert_eq!(timeline.sequence(), before);
    timeline.state.redo_active_edit();
    assert_eq!(timeline.sequence(), pasted);
    timeline.state.save_all().unwrap();
    let reopened = donder_project_io::load_project(&timeline.root).unwrap();
    assert_eq!(reopened.project.sequence(&timeline.id).unwrap(), &pasted);
}

#[test]
fn automation_row_placement_survives_target_reorder_and_history_without_rebinding() {
    let timeline = Timeline::new();
    let before = timeline.sequence();
    let target = timeline.targets()[1].clone();
    let original = before
        .automation_clips
        .iter()
        .find(|clip| clip.id.0 == timeline.automation)
        .unwrap();
    timeline.edit(SequenceGuiEdit::MoveAutomationClip {
        id: timeline.automation,
        start_seconds: original.start.as_seconds_f32(),
        row_target: target.clone(),
    });
    let moved = timeline.sequence();
    let clip = moved
        .automation_clips
        .iter()
        .find(|clip| clip.id.0 == timeline.automation)
        .unwrap();
    assert_eq!(clip.row_target.fixture.0, target.fixture);
    assert_eq!(clip.bindings, original.bindings);
    let session = timeline.state.project_session().unwrap();
    let setup = session
        .project
        .setup(session.project.root().setup.id())
        .unwrap();
    let layout = session.project.layout(setup.layout.id()).unwrap();
    let root_group = layout.fixtures[0].id.0;
    let result = timeline.state.apply_gui_edit(
        GuiDocumentRequest {
            project_revision: timeline.state.snapshot().project_revision,
            path: layout.id.0.document().to_string(),
            view: DocumentViewId::Layout,
            object_key: Some(layout.id.0.root_source().object().into()),
            owned_path: Vec::new(),
        },
        GuiEditCommand::Layout {
            edit: LayoutGuiEdit::MoveMember {
                id: target.fixture,
                from: Some(root_group),
                to: Some(root_group),
                before: None,
            },
        },
    );
    assert!(
        matches!(result.document, GuiDocument::Layout { .. }),
        "{:?}",
        result.document
    );
    assert_ne!(timeline.targets()[1].fixture, target.fixture);
    assert_eq!(timeline.sequence(), moved);
    timeline.state.undo_active_edit();
    assert_eq!(timeline.targets()[1].fixture, target.fixture);
    timeline.state.undo_active_edit();
    assert_eq!(timeline.sequence(), before);
    timeline.state.redo_active_edit();
    assert_eq!(timeline.sequence(), moved);
}

#[test]
fn out_of_bounds_paste_is_atomic_and_does_not_consume_history() {
    let timeline = Timeline::new();
    timeline.selection_edit(SequenceSelectionEdit::Copy {
        selection: timeline.selection(),
    });
    let before = timeline.sequence();
    let SequenceSelection::Clips { effect_ids, .. } = timeline.paste(1) else {
        panic!("clip selection missing")
    };
    timeline.selection_edit(SequenceSelectionEdit::Copy {
        selection: SequenceSelection::Clips {
            effect_ids: vec![timeline.effect, effect_ids[0]],
            automation_ids: vec![],
        },
    });
    let session = timeline.state.project_session().unwrap();
    let rejected = timeline.state.apply_sequence_selection_edit(
        timeline.request(),
        SequenceSelectionEdit::Paste {
            anchor: SequencePasteAnchor {
                lane: Some(timeline.targets().len() as u32 - 1),
                time_seconds: 0.0,
            },
        },
    );
    assert!(matches!(rejected.document, GuiDocument::Blocked { .. }));
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
    let selection = || SequenceSelection::Clips {
        effect_ids: vec![],
        automation_ids: vec![timeline.automation],
    };
    timeline.selection_edit(SequenceSelectionEdit::Copy {
        selection: selection(),
    });
    let SequenceSelection::Clips { automation_ids, .. } = timeline.paste(1) else {
        panic!("clip selection missing")
    };
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
    timeline.selection_edit(SequenceSelectionEdit::Cut {
        selection: selection(),
    });
    let SequenceSelection::Clips { automation_ids, .. } = timeline.paste(2) else {
        panic!("clip selection missing")
    };
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
        .setup(session.project.root().setup.id())
        .unwrap();
    let layout_id = setup.layout.id().clone();
    let request = GuiDocumentRequest {
        project_revision: timeline.state.snapshot().project_revision,
        path: layout_id.0.document().to_string(),
        view: DocumentViewId::Layout,
        object_key: Some(layout_id.0.root_source().object().into()),
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
    fixtures.push(crate::dto::GuiLayoutFixture {
        id: 100,
        name: "Pair".into(),
        description: None,
        kind: crate::dto::GuiLayoutFixtureKind::Group {
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
    let effect_target = || {
        timeline
            .sequence()
            .effects
            .iter()
            .find(|effect| effect.id.0 == timeline.effect)
            .unwrap()
            .target
            .fixture
            .0
    };
    let move_clip = |anchor_lane: usize, lane_delta| {
        timeline.selection_edit(SequenceSelectionEdit::MoveClips {
            effect_ids: vec![timeline.effect],
            automation_ids: vec![],
            time_delta_seconds: 0.0,
            anchor_lane: anchor_lane as u32,
            lane_delta,
        });
    };
    move_clip(0, 1);
    assert_eq!(effect_target(), first);
    // Moving up from the copy under `Pair` reaches `Pair`, not the lane above
    // the first copy.
    move_clip(pair + 1, -1);
    assert_eq!(effect_target(), 100);
}
