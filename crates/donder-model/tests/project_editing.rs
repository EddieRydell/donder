use std::time::Duration;

use donder_language::{DonderDuration, DonderTime};
use donder_model::{DonderProject, ProjectData, ProjectEdit, ProjectId, ProjectRoot};
use donder_model::{Layout, LayoutId};

use donder_model::{EditError, SequenceId};
use donder_runtime_types::Color;
use indexmap::IndexMap;

fn data() -> ProjectData {
    ProjectData {
        root: ProjectRoot {
            description: None,
            id: ProjectId(0),
            layout: LayoutId(2),
            routes: vec![],
        },
        layouts: IndexMap::from([(
            LayoutId(2),
            Layout {
                description: None,
                fixtures: vec![],
                root: vec![],
            },
        )]),
        controllers: IndexMap::new(),
        input_devices: IndexMap::new(),
        sequences: IndexMap::new(),
        consoles: IndexMap::new(),
        fixtures: IndexMap::new(),
        curves: IndexMap::new(),
        gradients: IndexMap::new(),
        scripts: IndexMap::new(),
        next_id: 4,
    }
}

fn color() -> Color {
    Color::BLACK
}

/// Put a new sequence of `duration` at `frame_rate` with the default composition.
fn add_sequence(
    project: &mut DonderProject,
    duration: DonderDuration,
    frame_rate: u32,
    color: Color,
) -> Result<SequenceId, EditError> {
    let id = SequenceId(project.new_id());
    project.put((
        id,
        donder_model::Sequence {
            description: None,
            duration,
            frame_rate,
            audio: donder_model::SequenceAudio::None,
            composition: donder_model::Composition {
                marks: vec![donder_model::MarkCollection {
                    id: donder_model::MarkCollectionId(0),
                    name: donder_language::object_name("marks"),
                    description: None,
                    display_color: color,
                    marks: Vec::new(),
                }],
                ..donder_model::Composition::default(color)
            },
        },
    ))?;
    Ok(id)
}

#[test]
fn geometry_cache_reuses_unchanged_inputs_and_rebuilds_changed_inputs() {
    use donder_language::DistanceSpan;
    use donder_model::{
        FixtureDefinition, FixtureDefinitionId, FixtureElement, FixtureElementId, FixtureShape,
        FixtureTransform,
    };
    use donder_model::{FixtureId, LayoutFixture, LayoutFixtureKind};

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
    let layout_id = data.root.layout;
    let layout = data.layouts.get_mut(&layout_id).unwrap();
    layout.fixtures.push(LayoutFixture {
        description: None,
        id: FixtureId(1),
        name: donder_language::object_name("fixture"),
        kind: LayoutFixtureKind::Fixture {
            definition: fixture_id,
            transform: FixtureTransform::default(),
        },
    });
    layout.root.push(FixtureId(1));
    let mut project = DonderProject::try_new(data).unwrap();
    let sequence_id = add_sequence(
        &mut project,
        DonderDuration(Duration::from_secs(3)),
        30,
        color(),
    )
    .unwrap();
    let before = project.clone();
    let original = before
        .view::<donder_model::Layout>(layout_id)
        .unwrap()
        .playback_geometry();
    let mut sequence = project
        .get::<donder_model::Sequence>(sequence_id)
        .unwrap()
        .clone();
    sequence.duration = DonderDuration(Duration::from_secs(4));
    project.put((sequence_id, sequence)).unwrap();
    assert!(std::ptr::eq(
        original,
        project
            .view::<donder_model::Layout>(layout_id)
            .unwrap()
            .playback_geometry()
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
        project
            .view::<donder_model::Layout>(layout_id)
            .unwrap()
            .playback_geometry()
    ));
    let accepted = project.clone();
    let current = accepted
        .view::<donder_model::Layout>(layout_id)
        .unwrap()
        .playback_geometry();
    let mut layout = project
        .get::<donder_model::Layout>(layout_id)
        .unwrap()
        .clone();
    layout.fixtures[0].name = donder_language::object_name("renamed fixture");
    project.put((layout_id, layout)).unwrap();
    assert!(!std::ptr::eq(
        current,
        project
            .view::<donder_model::Layout>(layout_id)
            .unwrap()
            .playback_geometry()
    ));

    let accepted = project.clone();
    let current = accepted
        .view::<donder_model::Layout>(layout_id)
        .unwrap()
        .playback_geometry();
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
        project
            .view::<donder_model::Layout>(layout_id)
            .unwrap()
            .playback_geometry()
    ));
}

#[test]
fn admission_rejects_a_missing_layout() {
    let mut invalid = data();
    invalid.root.layout = LayoutId(99);
    assert!(DonderProject::try_new(invalid).is_err());
    let project = DonderProject::try_new(data()).unwrap();
    assert!(project.get::<Layout>(project.root().layout).is_some());
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
    sequence.composition.marks[0]
        .marks
        .push(donder_model::Mark::at(DonderTime(Duration::from_secs(2))));
    project.put((id, sequence)).unwrap();
    // A shorter sequence drops the marks past its end.
    let mut shortened = project.get::<donder_model::Sequence>(id).unwrap().clone();
    shortened.duration = DonderDuration(Duration::from_secs(1));
    project.put((id, shortened)).unwrap();
    assert!(matches!(
        project.take_repairs().as_slice(),
        [donder_model::Repair {
            action: donder_model::GiveWay::Shrink,
            ..
        }]
    ));
    let before = project.clone();
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
    let named_id = LayoutId(project.new_id());
    let named_layout = Layout {
        description: None,
        fixtures: vec![],
        root: vec![],
    };
    let mut setup = project.root().clone();
    setup.layout = named_id;
    assert!(project.put(setup.clone()).is_err());
    assert_eq!(project, before);
    project
        .apply_edits([
            ProjectEdit::Put((named_id, named_layout.clone()).into()),
            ProjectEdit::Put(setup.into()),
        ])
        .unwrap();
    assert_eq!(
        project.reusable::<donder_model::Layout>().get(&named_id),
        Some(&named_layout)
    );
    assert!(before.get::<Layout>(named_id).is_none());
    assert_eq!(before.root().layout, LayoutId(2));
}

#[test]
fn a_failed_later_batch_request_rolls_back_earlier_changes() {
    let mut project = DonderProject::try_new(data()).unwrap();
    let before = project.clone();
    let layout_id = LayoutId(project.new_id());
    let layout = Layout {
        description: None,
        fixtures: vec![],
        root: vec![],
    };
    assert!(
        project
            .apply_edits([
                ProjectEdit::Put((layout_id, layout).into()),
                ProjectEdit::Remove(LayoutId(u32::MAX).into()),
            ])
            .is_err()
    );
    assert_eq!(project, before);
}
