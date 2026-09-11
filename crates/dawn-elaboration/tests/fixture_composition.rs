use dawn_elaboration::fixture::PreparedFixtureDefinitions;
use dawn_language::fixture::{
    FixtureDefinition, FixtureDefinitionError, FixtureDefinitionId, FixtureDefinitions,
    FixtureTransform, Pixel, PixelId,
};
use dawn_language::identity::{DocumentId, SourceIdentity};
use dawn_language::layout::{
    FixtureInstanceId, FixtureTarget, Layout, LayoutError, LayoutFixture, LayoutFixtureKind,
    LayoutId,
};
use dawn_language::values::{Distance, DistanceSpan, Point3};

fn identity(key: &str) -> SourceIdentity {
    SourceIdentity::from_document(
        DocumentId::new(
            Default::default(),
            "fixtures/composition.fixture.dawn".into(),
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

fn pixel(id: u32, x: f32) -> Pixel {
    Pixel {
        id: PixelId(id),
        position: translation(x, 0.0).position,
        diameter: DistanceSpan::from_meters(0.01),
    }
}

fn instance(id: u32, definition: &str) -> LayoutFixture {
    LayoutFixture {
        id: FixtureInstanceId(id),
        name: format!("Fixture {id}"),
        kind: LayoutFixtureKind::Fixture {
            definition: definition_id(definition),
            transform: translation(id as f32, 0.0),
        },
    }
}

fn definitions(items: &[(&str, Vec<Pixel>)]) -> FixtureDefinitions {
    FixtureDefinitions {
        definitions: items
            .iter()
            .map(|(name, parts)| {
                (
                    definition_id(name),
                    FixtureDefinition {
                        pixels: parts.clone(),
                    },
                )
            })
            .collect(),
    }
}

#[test]
fn reordering_changes_order_without_changing_pixel_identity() {
    let mut definitions = definitions(&[("pair", vec![pixel(7, 1.0), pixel(3, 2.0)])]);
    let before = PreparedFixtureDefinitions::prepare(&definitions).unwrap();
    definitions
        .definitions
        .get_mut(&definition_id("pair"))
        .unwrap()
        .pixels
        .reverse();
    let after = PreparedFixtureDefinitions::prepare(&definitions).unwrap();
    let before = before.pixels(&definition_id("pair")).unwrap();
    let after = after.pixels(&definition_id("pair")).unwrap();
    assert_eq!(before[0].id, after[1].id);
    assert_eq!(before[1].id, after[0].id);
    assert_eq!(before[0].position, after[1].position);
}

#[test]
fn layout_targets_remain_independent_after_definition_flattening() {
    let definitions = definitions(&[(
        "assembly",
        vec![pixel(7, 1.0), pixel(3, 2.0), pixel(4, 3.0), pixel(8, 4.0)],
    )]);
    let prepared = PreparedFixtureDefinitions::prepare(&definitions).unwrap();
    let layout = Layout {
        id: LayoutId(identity("layout")),
        fixtures: vec![LayoutFixture {
            id: FixtureInstanceId(100),
            name: "All".into(),
            kind: LayoutFixtureKind::Group {
                children: vec![instance(1, "assembly"), instance(2, "assembly")],
            },
        }],
    };
    let layout = prepared.prepare_layout(&layout).unwrap();
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
        assert_eq!(prepared.pixels(&selected[0].definition).unwrap().len(), 4);
    }
    assert_eq!(
        layout.target(&target(20)).unwrap_err(),
        LayoutError::MissingFixture(FixtureInstanceId(20))
    );
}

#[test]
fn duplicate_pixel_ids_are_rejected() {
    let duplicate = definitions(&[("a", vec![pixel(1, 0.0), pixel(1, 1.0)])]);
    assert_eq!(
        PreparedFixtureDefinitions::prepare(&duplicate).unwrap_err(),
        FixtureDefinitionError::DuplicatePixel {
            definition: definition_id("a"),
            pixel: PixelId(1)
        }
    );
}

#[test]
fn empty_layouts_and_definitions_are_valid_authoring_states() {
    let definitions = definitions(&[("empty", vec![])]);
    let prepared = PreparedFixtureDefinitions::prepare(&definitions).unwrap();
    assert!(prepared.pixels(&definition_id("empty")).unwrap().is_empty());
    let layout = Layout {
        id: LayoutId(identity("layout")),
        fixtures: vec![],
    };
    assert!(
        prepared
            .prepare_layout(&layout)
            .unwrap()
            .instances
            .is_empty()
    );
}
