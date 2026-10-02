use std::collections::BTreeMap;
use std::time::Duration;

use donder_language::identity::{DocumentId, ObjectIdentity, OwnedObjectSlot, SourceIdentity};
use donder_language::layout::{Layout, LayoutId};
use donder_language::model::{
    DonderProject, ProjectData, ProjectDefinitionStores, ProjectEdit, ProjectId, ProjectRoot,
};
use donder_language::ownership::{
    ValueSource,
    edit::{OwnershipSite, add_sequence, make_reusable, use_existing},
};
use donder_language::patch::{Patch, PatchId};
use donder_language::setup::{Setup, SetupId};
use donder_language::values::{Color, DonderDuration, DonderTime};
use indexmap::IndexMap;

fn identity(document: &str, object: &str) -> SourceIdentity {
    SourceIdentity::from_document(
        DocumentId::new(uuid::Uuid::nil(), document.into()),
        object.into(),
    )
}

fn data() -> ProjectData {
    let source = identity("project.donder", "project");
    let setup = ObjectIdentity::from(source.clone()).owned(OwnedObjectSlot::Setup);
    ProjectData {
        root: ProjectRoot {
            id: ProjectId(source),
            setup: ValueSource::Inline(Box::new(Setup {
                id: SetupId(setup.clone()),
                layout: ValueSource::Inline(Box::new(Layout {
                    id: LayoutId(setup.owned(OwnedObjectSlot::Layout)),
                    fixtures: vec![],
                })),
                patch: ValueSource::Inline(Box::new(Patch {
                    id: PatchId(setup.owned(OwnedObjectSlot::Patch)),
                    routes: vec![],
                })),
                controllers: vec![],
            })),
            sequences: vec![],
        },
        setups: IndexMap::new(),
        layouts: IndexMap::new(),
        patches: IndexMap::new(),
        controllers: IndexMap::new(),
        sequences: IndexMap::new(),
        definitions: ProjectDefinitionStores::default(),
    }
}

fn color() -> Color {
    Color::BLACK
}

#[test]
fn geometry_cache_reuses_unchanged_inputs_and_rebuilds_changed_inputs() {
    use donder_language::fixture::{
        FixtureDefinition, FixtureDefinitionId, FixtureElement, FixtureElementId, FixtureShape,
        FixtureTransform,
    };
    use donder_language::layout::{FixtureInstanceId, LayoutFixture, LayoutFixtureKind};
    use donder_language::values::DistanceSpan;

    let fixture_id = FixtureDefinitionId(identity("fixture.donder", "fixture"));
    let mut fixture = FixtureDefinition {
        elements: vec![FixtureElement {
            id: FixtureElementId(1),
            name: "pixel".into(),
            transform: FixtureTransform::default(),
            diameter: DistanceSpan::from_meters(0.01),
            reverse: false,
            shape: FixtureShape::Pixel,
        }],
    };
    let mut data = data();
    data.definitions
        .fixtures
        .definitions
        .insert(fixture_id.clone(), fixture.clone());
    let ValueSource::Inline(setup) = &mut data.root.setup else {
        panic!("test setup is inline")
    };
    let ValueSource::Inline(layout) = &mut setup.layout else {
        panic!("test layout is inline")
    };
    let layout_id = layout.id.clone();
    layout.fixtures.push(LayoutFixture {
        id: FixtureInstanceId(1),
        name: "fixture".into(),
        kind: LayoutFixtureKind::Fixture {
            definition: ValueSource::Reference(fixture_id.clone()),
            transform: FixtureTransform::default(),
        },
    });
    let mut project = DonderProject::try_new(data).unwrap();
    let sequence_id = add_sequence(
        &mut project,
        DonderDuration(Duration::from_secs(3)),
        30,
        color(),
    )
    .unwrap();
    let before = project.clone();
    let original = before.playback_geometry(&layout_id).unwrap();
    let mut sequence = project.sequence(&sequence_id).unwrap().clone();
    sequence.duration = DonderDuration(Duration::from_secs(4));
    project.replace_sequence(&sequence_id, sequence).unwrap();
    assert!(std::ptr::eq(
        original,
        project.playback_geometry(&layout_id).unwrap()
    ));

    fixture.elements[0].shape = FixtureShape::Line {
        length: 1.0,
        count: 2,
    };
    project
        .apply_edits([ProjectEdit::SetFixtureDefinition {
            id: fixture_id.clone(),
            value: fixture.clone(),
        }])
        .unwrap();
    assert!(!std::ptr::eq(
        original,
        project.playback_geometry(&layout_id).unwrap()
    ));
    let accepted = project.clone();
    let current = accepted.playback_geometry(&layout_id).unwrap();
    let mut layout = project.layout(&layout_id).unwrap().clone();
    layout.fixtures[0].name = "renamed fixture".into();
    project.replace_layout(&layout_id, layout).unwrap();
    assert!(!std::ptr::eq(
        current,
        project.playback_geometry(&layout_id).unwrap()
    ));

    let accepted = project.clone();
    let current = accepted.playback_geometry(&layout_id).unwrap();
    fixture.elements[0].shape = FixtureShape::Line {
        length: 1.0,
        count: 0,
    };
    assert!(
        project
            .apply_edits([ProjectEdit::SetFixtureDefinition {
                id: fixture_id,
                value: fixture,
            }])
            .is_err()
    );
    assert!(std::ptr::eq(
        current,
        project.playback_geometry(&layout_id).unwrap()
    ));
}

#[test]
fn admission_rejects_missing_references_and_accepts_owned_setup() {
    let mut invalid = data();
    invalid.root.setup =
        ValueSource::Reference(SetupId(identity("setup.donder", "missing").into()));
    assert!(DonderProject::try_new(invalid).is_err());
    let project = DonderProject::try_new(data()).unwrap();
    assert!(project.setup(project.root().setup.id()).is_some());
}

#[test]
fn rejected_sequence_edits_preserve_the_accepted_project() {
    let mut project = DonderProject::try_new(data()).unwrap();
    let id = add_sequence(
        &mut project,
        DonderDuration(Duration::from_secs(3)),
        30,
        color(),
    )
    .unwrap();
    let mut sequence = project.sequence(&id).unwrap().clone();
    sequence.mark_collections[0]
        .marks
        .push(DonderTime(Duration::from_secs(2)));
    project.replace_sequence(&id, sequence).unwrap();
    let before = project.clone();
    let mut shortened = project.sequence(&id).unwrap().clone();
    shortened.duration = DonderDuration(Duration::from_secs(1));
    assert!(project.replace_sequence(&id, shortened).is_err());
    assert_eq!(project, before);
    assert!(
        add_sequence(
            &mut project,
            DonderDuration(Duration::from_secs(1)),
            1001,
            color()
        )
        .is_err()
    );
    assert_eq!(project, before);
}

#[test]
fn related_replacements_commit_together_and_do_not_change_prior_snapshots() {
    let mut project = DonderProject::try_new(data()).unwrap();
    let before = project.clone();
    let named_layout = Layout {
        id: LayoutId(identity("layout.donder", "layout").into()),
        fixtures: vec![],
    };
    let mut setup = project.setup(project.root().setup.id()).unwrap().clone();
    setup.layout = ValueSource::Reference(named_layout.id.clone());
    assert!(
        project
            .replace_setup(&setup.id.clone(), setup.clone())
            .is_err()
    );
    assert_eq!(project, before);
    project
        .apply_edits([
            ProjectEdit::InsertLayout(named_layout.clone()),
            ProjectEdit::ReplaceSetup {
                id: setup.id.clone(),
                value: setup,
            },
        ])
        .unwrap();
    assert_eq!(
        project.reusable_layouts().get(&named_layout.id),
        Some(&named_layout)
    );
    assert!(before.reusable_layouts().is_empty());
    assert!(
        before
            .setup(before.root().setup.id())
            .unwrap()
            .layout
            .inline()
            .is_some()
    );
}

#[test]
fn a_failed_later_batch_request_rolls_back_earlier_changes() {
    let mut project = DonderProject::try_new(data()).unwrap();
    let before = project.clone();
    let layout = Layout {
        id: LayoutId(identity("layout.donder", "layout").into()),
        fixtures: vec![],
    };
    assert!(
        project
            .apply_edits([
                ProjectEdit::InsertLayout(layout.clone()),
                ProjectEdit::InsertLayout(layout),
            ])
            .is_err()
    );
    assert_eq!(project, before);
}

#[test]
fn rejected_ownership_change_does_not_partially_promote_or_replace() {
    let mut project = DonderProject::try_new(data()).unwrap();
    let setup = project.root().setup.id().clone();
    let source = identity("layout.donder", "layout");
    make_reusable(
        &mut project,
        &OwnershipSite::SetupLayout(setup.clone()),
        source.clone(),
    )
    .unwrap();
    let before = project.clone();
    assert!(make_reusable(&mut project, &OwnershipSite::SetupLayout(setup), source).is_err());
    assert_eq!(project, before);
    assert!(
        use_existing(
            &mut project,
            &OwnershipSite::ProjectSequence(0),
            identity("sequence.donder", "missing")
        )
        .is_err()
    );
    assert_eq!(project, before);
}

#[test]
fn membership_validation_failure_rolls_back_a_completed_replacement() {
    let mut project = DonderProject::try_new(data()).unwrap();
    add_sequence(
        &mut project,
        DonderDuration(Duration::from_secs(1)),
        30,
        color(),
    )
    .unwrap();
    add_sequence(
        &mut project,
        DonderDuration(Duration::from_secs(1)),
        30,
        color(),
    )
    .unwrap();
    let reusable = identity("sequence.donder", "sequence");
    make_reusable(
        &mut project,
        &OwnershipSite::ProjectSequence(0),
        reusable.clone(),
    )
    .unwrap();
    let before = project.clone();
    let error =
        use_existing(&mut project, &OwnershipSite::ProjectSequence(1), reusable).unwrap_err();
    assert!(error.contains("Sequence appears more than once"));
    assert_eq!(project, before);
}

#[test]
fn remap_collisions_are_rejected_before_losing_document_objects() {
    let mut project = DonderProject::try_new(data()).unwrap();
    let setup = project.root().setup.id().clone();
    let source = identity("layout.donder", "layout");
    make_reusable(
        &mut project,
        &OwnershipSite::SetupLayout(setup),
        source.clone(),
    )
    .unwrap();
    let before = project.clone();
    let remaps = BTreeMap::from([(
        source.document_id().clone(),
        project.root().id.0.document_id().clone(),
    )]);
    assert!(donder_language::source_remap::remap_document_paths(&mut project, &remaps).is_err());
    assert_eq!(project, before);
}
