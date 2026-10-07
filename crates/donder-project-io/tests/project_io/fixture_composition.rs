use crate::common;

use camino::Utf8Path;
use donder_language::fixture::FixtureElementId;
use donder_language::layout::{FixtureInstanceId, LayoutFixtureKind};
use donder_project_io::{export_project, load_project, save_project};

const ASSEMBLY: &str = r#"
FixtureDefinition assembly {
  description: none,
  shapes: [
    Shape {
      name: first,
      diameter: 0.01m,
      reverse: false,
      transform: Transform { position: (1m, 0m, 0m), rotation: (0.0, 0.0, 0.0), scale: (1.0, 1.0, 1.0) },
      geometry: Pixel,
    },
    Shape {
      name: second,
      diameter: 0.02m,
      reverse: false,
      transform: Transform { position: (2m, 0m, 0m), rotation: (0.0, 0.0, 0.0), scale: (1.0, 1.0, 1.0) },
      geometry: Pixel,
    },
  ],
}
"#;

#[test]
fn inline_definitions_keep_source_ownership_and_pixel_order() {
    let (_temporary, root) = common::starter_copy();
    let root = root.as_path();
    let fixture_path = root.join("fixtures/vertical.data.donder");
    let mut source = std::fs::read_to_string(&fixture_path).unwrap();
    source.push_str(ASSEMBLY);
    std::fs::write(&fixture_path, source).unwrap();
    // The extra definition is indexed even though no layout uses it yet.
    let mut loaded = load_project(root).unwrap();
    let (id, mut definition) = loaded
        .project
        .definitions()
        .fixtures
        .definitions
        .iter()
        .find(|(id, _)| id.0.object() == "assembly")
        .map(|(id, definition)| (id.clone(), definition.clone()))
        .unwrap();
    assert_eq!(
        id.0.document(),
        Utf8Path::new("fixtures/vertical.data.donder")
    );
    assert_eq!(definition.elements[0].name.as_str(), "first");
    // Shape identities follow document order, so a reorder renumbers them.
    definition.elements.swap(0, 1);
    for (index, element) in definition.elements.iter_mut().enumerate() {
        element.id = FixtureElementId(index as u32 + 1);
    }
    loaded
        .project
        .apply_edits([donder_language::model::ProjectEdit::SetFixtureDefinition {
            id,
            value: definition,
        }])
        .unwrap();
    save_project(&loaded).unwrap();
    let saved = load_project(root).unwrap();
    assert_eq!(saved.project, loaded.project);
    let definition = saved
        .project
        .definitions()
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
            .map(|part| part.name.as_str())
            .collect::<Vec<_>>(),
        ["second", "first"]
    );
    let layout = saved.project.reusable_layouts().values().next().unwrap();
    assert!(matches!(
        layout.fixture(FixtureInstanceId(2)).unwrap().kind,
        LayoutFixtureKind::Fixture { .. }
    ));
}

#[test]
fn all_shape_parameters_round_trip_in_authored_order() {
    use donder_language::fixture::{FixtureShape, GridAxis, GridCorner};
    let mut session = common::load_project(&common::starter_root());
    let (id, mut fixture) = session
        .project
        .definitions()
        .fixtures
        .definitions
        .iter()
        .next()
        .map(|(id, definition)| (id.clone(), definition.clone()))
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
            element.id = FixtureElementId(index as u32 + 1);
            element.name = donder_language::names::object_name(&format!("shape_{index}"));
            element.shape = shape;
            element.reverse = index % 2 == 0;
            element
        })
        .collect();
    session
        .project
        .apply_edits([donder_language::model::ProjectEdit::SetFixtureDefinition {
            id,
            value: fixture,
        }])
        .unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    export_project(&session, root).unwrap();
    assert_eq!(load_project(root).unwrap().project, session.project);
}
