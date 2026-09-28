use camino::{Utf8Path, Utf8PathBuf};
use donder_language::layout::{FixtureInstanceId, FixtureTarget, LayoutFixtureKind};
use donder_project_io::{export_project, load_package, save_project};

fn starter() -> donder_project_io::ProjectSession {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    load_package(&root).unwrap().session
}

fn export_starter(session: &donder_project_io::ProjectSession, root: &Utf8Path) {
    export_project(session, root).unwrap();
    let source = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    for name in ["donder-package.json", "donder.lock"] {
        std::fs::copy(source.join(name), root.join(name)).unwrap();
    }
    for path in donder_package::PackageManifest::read(&source)
        .unwrap()
        .assets
        .keys()
    {
        let destination = root.join(path);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::copy(source.join(path), destination).unwrap();
    }
}

#[test]
fn starter_layout_targets_and_led_routes_round_trip() {
    let session = starter();
    let layout = session.project.layouts.values().next().unwrap();
    let counts = session.project.definitions.fixtures.pixel_counts().unwrap();
    assert_eq!(
        layout
            .target_pixel_count(
                &FixtureTarget {
                    layout: layout.id.clone(),
                    fixture: FixtureInstanceId(1001)
                },
                &counts
            )
            .unwrap(),
        30 * 113
    );
    for id in 1..=30 {
        assert_eq!(
            layout
                .target_pixel_count(
                    &FixtureTarget {
                        layout: layout.id.clone(),
                        fixture: FixtureInstanceId(id)
                    },
                    &counts
                )
                .unwrap(),
            113
        );
    }
    let patch = session.project.patches.values().next().unwrap();
    assert_eq!(patch.routes.len(), 30);
    assert!(
        patch
            .routes
            .iter()
            .all(|route| route.pixels.is_none() && route.encoding.channel_order() == [1, 0, 2])
    );
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    export_starter(&session, root);
    let loaded = load_package(root).unwrap().session;
    assert_eq!(loaded.project, session.project);
}

#[test]
fn inline_definitions_keep_source_ownership_and_pixel_order() {
    let session = starter();
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    export_starter(&session, root);
    let fixture_path = root.join("fixtures/vertical.fixture.donder");
    let mut source = std::fs::read_to_string(&fixture_path).unwrap();
    source.push_str("\nassembly:\n  type: fixture\n  elements:\n  - id: 90\n    name: First\n    reverse: false\n    shape: {type: pixel}\n    diameter: 0.01\n    transform: {position: {x: 1, y: 0, z: 0}}\n  - id: 10\n    name: Second\n    reverse: false\n    shape: {type: pixel}\n    diameter: 0.02\n    transform: {position: {x: 2, y: 0, z: 0}}\n");
    std::fs::write(&fixture_path, source).unwrap();
    // The extra definition is indexed even though no layout uses it yet.
    let mut loaded = load_package(root).unwrap().session;
    let (id, definition) = loaded
        .project
        .definitions
        .fixtures
        .definitions
        .iter_mut()
        .find(|(id, _)| id.0.object() == "assembly")
        .unwrap();
    assert_eq!(
        id.0.document(),
        Utf8Path::new("fixtures/vertical.fixture.donder")
    );
    assert_eq!(definition.elements[0].id.0, 90);
    definition.elements.swap(0, 1);
    save_project(&loaded).unwrap();
    let saved = load_package(root).unwrap().session;
    assert_eq!(saved.project, loaded.project);
    let definition = saved
        .project
        .definitions
        .fixtures
        .definitions
        .iter()
        .find(|(id, _)| id.0.object() == "assembly")
        .unwrap()
        .1;
    assert_eq!(
        definition
            .elements
            .iter()
            .map(|part| part.id.0)
            .collect::<Vec<_>>(),
        [10, 90]
    );
    let layout = saved.project.layouts.values().next().unwrap();
    assert!(matches!(
        layout.fixture(FixtureInstanceId(1)).unwrap().kind,
        LayoutFixtureKind::Fixture { .. }
    ));
}

#[test]
fn all_shape_parameters_round_trip_in_authored_order() {
    use donder_language::fixture::{FixtureElementId, FixtureShape, GridAxis, GridCorner};
    let mut session = starter();
    let fixture = session
        .project
        .definitions
        .fixtures
        .definitions
        .values_mut()
        .next()
        .unwrap();
    let template = fixture.elements[0].clone();
    let shapes = vec![
        FixtureShape::Pixel,
        FixtureShape::Line {
            length: 3.0,
            count: 7,
        },
        FixtureShape::Polyline {
            points: vec![
                Default::default(),
                template.transform.position,
                donder_language::values::Point3 {
                    x: donder_language::values::Distance::from_meters(2.0),
                    ..Default::default()
                },
            ],
            count: 9,
        },
        FixtureShape::Arc {
            radius: 2.0,
            start_degrees: 30.0,
            sweep_degrees: -180.0,
            count: 8,
            closed: false,
        },
        FixtureShape::Arc {
            radius: 3.0,
            start_degrees: 45.0,
            sweep_degrees: 360.0,
            count: 12,
            closed: true,
        },
        FixtureShape::Grid {
            columns: 4,
            rows: 3,
            width: 3.0,
            height: 2.0,
            axis: GridAxis::Columns,
            corner: GridCorner::TopRight,
            serpentine: true,
        },
    ];
    fixture.elements = shapes
        .into_iter()
        .enumerate()
        .map(|(index, shape)| {
            let mut element = template.clone();
            element.id = FixtureElementId(100 - index as u32);
            element.name = format!("Shape {index}");
            element.shape = shape;
            element.reverse = index % 2 == 0;
            element
        })
        .collect();
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    export_starter(&session, root);
    let loaded = load_package(root).unwrap().session;
    assert_eq!(loaded.project, session.project);
    save_project(&loaded).unwrap();
    assert_eq!(load_package(root).unwrap().session.project, session.project);
}
