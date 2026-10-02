use donder_language::fixture::{
    FixtureDefinition, FixtureDefinitionError, FixtureDefinitionId, FixtureDefinitions,
    FixtureElement, FixtureElementId, FixtureShape, FixtureTransform, GridAxis, GridCorner,
};
use donder_language::geometry::PreparedFixtureDefinitions;
use donder_language::identity::{DocumentId, SourceIdentity};
use donder_language::layout::{
    FixtureInstanceId, FixtureTarget, Layout, LayoutError, LayoutFixture, LayoutFixtureKind,
    LayoutId,
};
use donder_language::values::{Distance, DistanceSpan, Point3};

fn identity(key: &str) -> SourceIdentity {
    SourceIdentity::from_document(
        DocumentId::new(
            Default::default(),
            "fixtures/composition.fixture.donder".into(),
        ),
        key.into(),
    )
}

fn definition_id(key: &str) -> FixtureDefinitionId {
    FixtureDefinitionId(identity(key))
}

fn translation(x: f32, y: f32) -> FixtureTransform {
    FixtureTransform {
        position: Point3 {
            x: Distance::from_meters(x),
            y: Distance::from_meters(y),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn pixel(id: u32, x: f32) -> FixtureElement {
    FixtureElement {
        id: FixtureElementId(id),
        name: format!("Pixel {id}"),
        reverse: false,
        shape: FixtureShape::Pixel,
        transform: translation(x, 0.0),
        diameter: DistanceSpan::from_meters(0.01),
    }
}

fn instance(id: u32, definition: &str) -> LayoutFixture {
    LayoutFixture {
        id: FixtureInstanceId(id),
        name: format!("Fixture {id}"),
        kind: LayoutFixtureKind::Fixture {
            definition: donder_language::fixture::FixtureSource::Reference(definition_id(
                definition,
            )),
            transform: translation(id as f32, 0.0),
        },
    }
}

fn definitions(items: &[(&str, Vec<FixtureElement>)]) -> FixtureDefinitions {
    FixtureDefinitions {
        definitions: items
            .iter()
            .map(|(name, parts)| {
                (
                    definition_id(name),
                    FixtureDefinition {
                        elements: parts.clone(),
                    },
                )
            })
            .collect(),
    }
}

#[test]
fn reordering_changes_order_without_changing_pixel_identity() {
    let mut definitions = definitions(&[("pair", vec![pixel(7, 1.0), pixel(3, 2.0)])]);
    let before = PreparedFixtureDefinitions::prepare(&definitions);
    definitions
        .definitions
        .get_mut(&definition_id("pair"))
        .unwrap()
        .elements
        .reverse();
    let after = PreparedFixtureDefinitions::prepare(&definitions);
    let before = before.pixels(&definition_id("pair")).unwrap();
    let after = after.pixels(&definition_id("pair")).unwrap();
    assert_eq!(before[0].element, after[1].element);
    assert_eq!(before[1].element, after[0].element);
    assert_eq!(before[0].position, after[1].position);
}

#[test]
fn layout_targets_remain_independent_after_definition_flattening() {
    let definitions = definitions(&[(
        "assembly",
        vec![pixel(7, 1.0), pixel(3, 2.0), pixel(4, 3.0), pixel(8, 4.0)],
    )]);
    let prepared = PreparedFixtureDefinitions::prepare(&definitions);
    let layout = Layout {
        id: LayoutId(identity("layout").into()),
        fixtures: vec![LayoutFixture {
            id: FixtureInstanceId(100),
            name: "All".into(),
            kind: LayoutFixtureKind::Group {
                children: vec![instance(1, "assembly"), instance(2, "assembly")],
            },
        }],
    };
    let layout = prepared.prepare_layout(&layout);
    let target = |fixture| FixtureTarget {
        layout: layout.id.clone(),
        fixture: FixtureInstanceId(fixture),
    };
    assert_eq!(
        layout
            .target(&target(100))
            .unwrap()
            .iter()
            .map(|instance| instance.id)
            .collect::<Vec<_>>(),
        vec![FixtureInstanceId(1), FixtureInstanceId(2)]
    );
    for id in [1, 2] {
        let selected = layout.target(&target(id)).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, FixtureInstanceId(id));
        assert_eq!(selected[0].pixels.len(), 4);
    }
    assert_eq!(
        layout.target(&target(20)).unwrap_err(),
        LayoutError::MissingFixture(FixtureInstanceId(20))
    );
}

#[test]
fn duplicate_element_ids_are_rejected() {
    let duplicate = definitions(&[("a", vec![pixel(1, 0.0), pixel(1, 1.0)])]);
    assert_eq!(
        duplicate.validate().unwrap_err(),
        FixtureDefinitionError::DuplicateElement {
            definition: definition_id("a"),
            element: FixtureElementId(1)
        }
    );
}

#[test]
fn authoring_validation_rejects_invalid_derived_coordinates() {
    let mut element = pixel(1, 0.0);
    element.shape = FixtureShape::Line {
        length: 1_500.0,
        count: 2,
    };
    element.transform.scale.x = 2.0;
    let fixture = FixtureDefinition {
        elements: vec![element],
    };
    assert!(matches!(
        fixture.validate_geometry(),
        Err(donder_language::fixture::FixtureGeometryError::InvalidElement(FixtureElementId(1)))
    ));
}

#[test]
fn empty_layouts_and_definitions_are_valid_authoring_states() {
    let definitions = definitions(&[("empty", vec![])]);
    let prepared = PreparedFixtureDefinitions::prepare(&definitions);
    assert!(prepared.pixels(&definition_id("empty")).unwrap().is_empty());
    let layout = Layout {
        id: LayoutId(identity("layout").into()),
        fixtures: vec![],
    };
    assert!(prepared.prepare_layout(&layout).instances.is_empty());
}

fn shape_pixels(shape: FixtureShape) -> Vec<donder_language::geometry::PreparedPixel> {
    let mut element = pixel(7, 0.0);
    element.shape = shape;
    donder_language::geometry::element_pixels(&element).unwrap()
}

fn assert_positions(pixels: &[donder_language::geometry::PreparedPixel], expected: &[[f32; 3]]) {
    assert_eq!(pixels.len(), expected.len());
    for (pixel, expected) in pixels.iter().zip(expected) {
        assert!(
            (pixel.position - glam::Vec3::from_array(*expected)).length() < 0.00001,
            "{:?} != {expected:?}",
            pixel.position
        );
    }
}

#[test]
fn lines_and_polylines_space_pixels_along_the_whole_path() {
    assert_positions(
        &shape_pixels(FixtureShape::Line {
            length: 4.0,
            count: 3,
        }),
        &[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [4.0, 0.0, 0.0]],
    );
    let point = |x, y| translation(x, y).position;
    assert_positions(
        &shape_pixels(FixtureShape::Polyline {
            points: vec![
                point(0.0, 0.0),
                point(1.0, 0.0),
                point(1.0, 0.0),
                point(1.0, 3.0),
            ],
            count: 5,
        }),
        &[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 2.0, 0.0],
            [1.0, 3.0, 0.0],
        ],
    );
    assert_positions(
        &shape_pixels(FixtureShape::Line {
            length: 4.0,
            count: 1,
        }),
        &[[0.0, 0.0, 0.0]],
    );
}

#[test]
fn arcs_include_endpoints_and_circles_do_not_repeat_the_start() {
    assert_positions(
        &shape_pixels(FixtureShape::Arc {
            radius: 2.0,
            start_degrees: 0.0,
            sweep_degrees: 180.0,
            count: 3,
            closed: false,
        }),
        &[[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [-2.0, 0.0, 0.0]],
    );
    assert_positions(
        &shape_pixels(FixtureShape::Arc {
            radius: 2.0,
            start_degrees: 0.0,
            sweep_degrees: -360.0,
            count: 4,
            closed: true,
        }),
        &[
            [2.0, 0.0, 0.0],
            [0.0, -2.0, 0.0],
            [-2.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
        ],
    );
}

#[test]
fn grid_traversal_and_reversal_preserve_pixel_identity() {
    let mut element = pixel(7, 10.0);
    element.shape = FixtureShape::Grid {
        columns: 3,
        rows: 2,
        width: 2.0,
        height: 1.0,
        axis: GridAxis::Rows,
        corner: GridCorner::TopRight,
        serpentine: true,
    };
    let forward = donder_language::geometry::element_pixels(&element).unwrap();
    assert_positions(
        &forward,
        &[
            [12.0, 1.0, 0.0],
            [11.0, 1.0, 0.0],
            [10.0, 1.0, 0.0],
            [10.0, 0.0, 0.0],
            [11.0, 0.0, 0.0],
            [12.0, 0.0, 0.0],
        ],
    );
    element.reverse = true;
    let reversed = donder_language::geometry::element_pixels(&element).unwrap();
    for (a, b) in forward.iter().zip(reversed.iter().rev()) {
        assert_eq!(
            (a.element, a.ordinal, a.position),
            (b.element, b.ordinal, b.position)
        );
    }
    assert_positions(
        &shape_pixels(FixtureShape::Grid {
            columns: 2,
            rows: 3,
            width: 1.0,
            height: 2.0,
            axis: GridAxis::Columns,
            corner: GridCorner::BottomLeft,
            serpentine: true,
        }),
        &[
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 2.0, 0.0],
            [1.0, 2.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
        ],
    );
}

#[test]
fn malformed_or_unbounded_shapes_are_rejected_before_expansion() {
    for shape in [
        FixtureShape::Line {
            length: 1.0,
            count: 0,
        },
        FixtureShape::Line {
            length: f32::NAN,
            count: 3,
        },
        FixtureShape::Polyline {
            points: vec![Point3::default(); 2],
            count: 2,
        },
        FixtureShape::Arc {
            radius: 1.0,
            start_degrees: 0.0,
            sweep_degrees: 90.0,
            count: 4,
            closed: true,
        },
        FixtureShape::Grid {
            columns: u32::MAX,
            rows: 2,
            width: 1.0,
            height: 1.0,
            axis: GridAxis::Rows,
            corner: GridCorner::BottomLeft,
            serpentine: false,
        },
    ] {
        let mut element = pixel(1, 0.0);
        element.shape = shape;
        assert!(
            definitions(&[("invalid", vec![element])])
                .validate()
                .is_err()
        );
    }
}
