use std::time::Duration;

use donder_language::{DonderDuration, DonderTime};
use donder_model::{DonderProject, ProjectData, ProjectEdit, ProjectId, ProjectRoot};
use donder_model::{Layout, LayoutId};
use donder_model::{ObjectId, SequenceId};
use donder_model::{Owned, OwnershipSite, add_sequence, make_reusable, use_existing};
use donder_model::{Patch, PatchId};
use donder_model::{Setup, SetupId};
use donder_runtime_types::Color;
use indexmap::IndexMap;

fn data() -> ProjectData {
    ProjectData {
        root: ProjectRoot {
            description: None,
            id: ProjectId(0),
            setup: Owned::Inline(Box::new(Setup {
                description: None,
                id: SetupId(1),
                layout: Owned::Inline(Box::new(Layout {
                    description: None,
                    id: LayoutId(2),
                    fixtures: vec![],
                    root: vec![],
                })),
                patch: Owned::Inline(Box::new(Patch {
                    description: None,
                    id: PatchId(3),
                    routes: vec![],
                })),
                controllers: vec![],
                input_devices: vec![],
            })),
            sequences: vec![],
            consoles: vec![],
        },
        setups: IndexMap::new(),
        layouts: IndexMap::new(),
        patches: IndexMap::new(),
        controllers: IndexMap::new(),
        input_devices: IndexMap::new(),
        sequences: IndexMap::new(),
        consoles: IndexMap::new(),
        fixtures: IndexMap::new(),
        curves: IndexMap::new(),
        gradients: IndexMap::new(),
        effects: IndexMap::new(),
        operators: IndexMap::new(),
        next_id: 4,
    }
}

fn color() -> Color {
    Color::BLACK
}

#[test]
fn geometry_cache_reuses_unchanged_inputs_and_rebuilds_changed_inputs() {
    use donder_language::DistanceSpan;
    use donder_model::{
        FixtureDefinition, FixtureDefinitionId, FixtureElement, FixtureElementId, FixtureShape,
        FixtureTransform,
    };
    use donder_model::{FixtureInstanceId, LayoutFixture, LayoutFixtureKind};

    let mut fixture = FixtureDefinition {
        description: None,
        elements: vec![FixtureElement {
            id: FixtureElementId(1),
            name: donder_language::object_name("pixel"),
            transform: FixtureTransform::default(),
            diameter: DistanceSpan::from_meters(0.01),
            reverse: false,
            shape: FixtureShape::Pixel,
        }],
    };
    let mut data = data();
    let fixture_id = FixtureDefinitionId(data.next_id);
    data.next_id += 1;
    data.fixtures.insert(fixture_id, fixture.clone());
    let Owned::Inline(setup) = &mut data.root.setup else {
        panic!("test setup is inline")
    };
    let Owned::Inline(layout) = &mut setup.layout else {
        panic!("test layout is inline")
    };
    let layout_id = layout.id;
    layout.fixtures.push(LayoutFixture {
        description: None,
        id: FixtureInstanceId(1),
        name: donder_language::object_name("fixture"),
        kind: LayoutFixtureKind::Fixture {
            definition: Owned::Reference(fixture_id),
            transform: FixtureTransform::default(),
        },
    });
    layout.root.push(FixtureInstanceId(1));
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
    let mut sequence = project
        .get::<donder_model::Sequence>(sequence_id)
        .unwrap()
        .clone();
    sequence.duration = DonderDuration(Duration::from_secs(4));
    project.put(sequence).unwrap();
    assert!(std::ptr::eq(
        original,
        project.playback_geometry(&layout_id).unwrap()
    ));

    fixture.elements[0].shape = FixtureShape::Line {
        length: 1.0,
        count: 2,
    };
    project
        .apply_edits([ProjectEdit::Put(donder_model::Object::FixtureDefinition(
            fixture_id,
            fixture.clone(),
        ))])
        .unwrap();
    assert!(!std::ptr::eq(
        original,
        project.playback_geometry(&layout_id).unwrap()
    ));
    let accepted = project.clone();
    let current = accepted.playback_geometry(&layout_id).unwrap();
    let mut layout = project
        .get::<donder_model::Layout>(layout_id)
        .unwrap()
        .clone();
    layout.fixtures[0].name = donder_language::object_name("renamed fixture");
    project.put(layout).unwrap();
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
            .apply_edits([ProjectEdit::Put(donder_model::Object::FixtureDefinition(
                fixture_id, fixture
            ))])
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
    invalid.root.setup = Owned::Reference(SetupId(99));
    assert!(DonderProject::try_new(invalid).is_err());
    let project = DonderProject::try_new(data()).unwrap();
    assert!(
        project
            .get::<donder_model::Setup>(*project.root().setup.id())
            .is_some()
    );
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
    let mut sequence = project.get::<donder_model::Sequence>(id).unwrap().clone();
    sequence.mark_collections[0]
        .marks
        .push(donder_model::Mark::at(DonderTime(Duration::from_secs(2))));
    project.put(sequence).unwrap();
    let before = project.clone();
    let mut shortened = project.get::<donder_model::Sequence>(id).unwrap().clone();
    shortened.duration = DonderDuration(Duration::from_secs(1));
    assert!(project.put(shortened).is_err());
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
        description: None,
        id: LayoutId(project.new_id()),
        fixtures: vec![],
        root: vec![],
    };
    let mut setup = project
        .get::<donder_model::Setup>(*project.root().setup.id())
        .unwrap()
        .clone();
    setup.layout = Owned::Reference(named_layout.id);
    assert!(project.put(setup.clone()).is_err());
    assert_eq!(project, before);
    project
        .apply_edits([
            ProjectEdit::Put(named_layout.clone().into()),
            ProjectEdit::Put(setup.into()),
        ])
        .unwrap();
    assert_eq!(
        project
            .reusable::<donder_model::Layout>()
            .get(&named_layout.id),
        Some(&named_layout)
    );
    assert!(before.reusable::<donder_model::Layout>().is_empty());
    assert!(
        before
            .get::<donder_model::Setup>(*before.root().setup.id())
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
        description: None,
        id: LayoutId(project.new_id()),
        fixtures: vec![],
        root: vec![],
    };
    assert!(
        project
            .apply_edits([
                ProjectEdit::Put(layout.into()),
                ProjectEdit::Remove(LayoutId(u32::MAX).into()),
            ])
            .is_err()
    );
    assert_eq!(project, before);
}

/// The project's sequence slot at `index`.
fn sequence_site(project: &DonderProject, index: usize) -> OwnershipSite {
    OwnershipSite::Member {
        owner: project.root().id.into(),
        member: (*project.root().sequences[index].id()).into(),
    }
}

#[test]
fn rejected_ownership_change_does_not_partially_promote_or_replace() {
    let mut project = DonderProject::try_new(data()).unwrap();
    let setup = *project.root().setup.id();
    let layout = OwnershipSite::Member {
        owner: setup.into(),
        member: (*project
            .get::<donder_model::Setup>(setup)
            .unwrap()
            .layout
            .id())
        .into(),
    };
    make_reusable(&mut project, &layout).unwrap();
    let before = project.clone();
    assert!(make_reusable(&mut project, &layout).is_err());
    assert_eq!(project, before);
    let site = OwnershipSite::Member {
        owner: project.root().id.into(),
        member: SequenceId(98).into(),
    };
    assert!(
        use_existing(
            &mut project,
            &site,
            ObjectId::Sequence(SequenceId(99)),
            &|_| None
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
    let site = sequence_site(&project, 0);
    let reusable = make_reusable(&mut project, &site).unwrap();
    let second = sequence_site(&project, 1);
    let before = project.clone();
    let error = use_existing(&mut project, &second, reusable, &|_| None).unwrap_err();
    assert!(matches!(
        error,
        donder_model::EditError::Invalid(donder_model::ProjectValidationError {
            problem: donder_model::ValidationProblem::Ownership(
                donder_model::OwnershipProblem::DuplicateSequence
            ),
            ..
        })
    ));
    assert_eq!(project, before);
}
