use crate::dto::*;
use crate::gui::{ResolvedGuiObject, blocked};
use donder_language::fixture::FixtureDefinitionId;
use donder_language::geometry::PreparedFixtureDefinitions;
use donder_language::layout::{LayoutFixture, LayoutFixtureKind, LayoutId};
use donder_project_io::{ProjectSession, SourceObjectKind};

pub(in crate::gui) fn project_fixture(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let (definition, name) = match resolved.owned_path.as_slice() {
        [] => {
            let Some(value) = session
                .project
                .definitions()
                .fixtures
                .definitions
                .get(&FixtureDefinitionId(resolved.identity.clone()))
            else {
                return blocked("Fixture was not found.", Vec::new());
            };
            (value, resolved.identity.object().to_string())
        }
        [parent @ .., GuiOwnedStep::Fixture { id }] => {
            let parent = parent.iter().fold(
                donder_language::identity::ObjectIdentity::from(resolved.identity.clone()),
                |address, step| address.owned(step.into()),
            );
            let Some(placement) = session
                .project
                .layout(&LayoutId(parent))
                .and_then(|layout| layout.fixture(donder_language::layout::FixtureInstanceId(*id)))
            else {
                return blocked("Fixture was not found.", Vec::new());
            };
            let LayoutFixtureKind::Fixture {
                definition: donder_language::fixture::FixtureSource::Inline(value),
                ..
            } = &placement.kind
            else {
                return blocked("Fixture is not owned inline.", Vec::new());
            };
            (value, placement.name.clone())
        }
        _ => return blocked("Unsupported owned fixture path.", Vec::new()),
    };
    let pixels = donder_language::geometry::prepare_geometry(definition);
    let pixels = pixels
        .iter()
        .enumerate()
        .map(|(index, pixel)| SpatialRenderPixel {
            owner: pixel.element.0,
            index: index as u32,
            position: point(pixel.position),
            diameter_meters: pixel.diameter_meters,
        })
        .collect();
    let handles: Vec<GuiFixtureHandle> = definition
        .elements
        .iter()
        .flat_map(|element| {
            donder_language::geometry::element_handles(element)
                .into_iter()
                .enumerate()
                .map(|(index, position)| GuiFixtureHandle {
                    element: element.id.0,
                    index: index as u32,
                    position: point(position),
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let mut plan = render_plan(pixels);
    for handle in &handles {
        plan.bounds.min_x_meters = plan.bounds.min_x_meters.min(handle.position.x_meters);
        plan.bounds.max_x_meters = plan.bounds.max_x_meters.max(handle.position.x_meters);
        plan.bounds.min_y_meters = plan.bounds.min_y_meters.min(handle.position.y_meters);
        plan.bounds.max_y_meters = plan.bounds.max_y_meters.max(handle.position.y_meters);
    }
    GuiDocument::Fixture {
        document: FixtureGuiDocument {
            name,
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            elements: definition
                .elements
                .iter()
                .map(crate::gui::fixture::gui_element)
                .collect(),
            handles,
            render_plan: plan,
        },
    }
}

pub(in crate::gui) fn project_layout(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let Some(layout) = session
        .project
        .layout(&LayoutId(resolved.object_identity()))
    else {
        return blocked("Layout was not found.", Vec::new());
    };
    let definitions = PreparedFixtureDefinitions::prepare(&session.project.definitions().fixtures);
    let prepared = definitions.prepare_layout(layout);
    let mut pixels = Vec::new();
    for instance in prepared.instances {
        let definition_pixels = &instance.pixels;
        pixels.extend(definition_pixels.iter().enumerate().map(|(index, pixel)| {
            SpatialRenderPixel {
                owner: instance.id.0,
                index: index as u32,
                position: point(instance.transform.transform_point3(pixel.position)),
                diameter_meters: pixel.diameter_meters,
            }
        }));
    }
    GuiDocument::Layout {
        document: LayoutGuiDocument {
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            fixtures: layout.fixtures.iter().map(fixture).collect(),
            available_fixtures: crate::gui::ownership::available_sources(
                session,
                resolved.identity.document_id(),
                &[SourceObjectKind::FixtureDefinition],
            ),
            render_plan: render_plan(pixels),
        },
    }
}

pub fn definition_ref(id: &FixtureDefinitionId) -> GuiObjectRef {
    ResolvedGuiObject {
        owned_path: Vec::new(),
        identity: id.0.clone(),
        kind: SourceObjectKind::FixtureDefinition,
    }
    .source_ref()
}

fn fixture(fixture: &LayoutFixture) -> GuiLayoutFixture {
    GuiLayoutFixture {
        id: fixture.id.0,
        name: fixture.name.clone(),
        kind: match &fixture.kind {
            LayoutFixtureKind::Fixture {
                definition,
                transform: value,
            } => GuiLayoutFixtureKind::Fixture {
                definition: match definition {
                    donder_language::fixture::FixtureSource::Inline(value) => {
                        GuiFixtureSource::Inline {
                            elements: value
                                .elements
                                .iter()
                                .map(crate::gui::fixture::gui_element)
                                .collect(),
                        }
                    }
                    donder_language::fixture::FixtureSource::Reference(id) => {
                        GuiFixtureSource::Reference {
                            source: definition_ref(id),
                        }
                    }
                },
                transform: crate::gui::fixture::gui_transform(value),
            },
            LayoutFixtureKind::Group { children } => GuiLayoutFixtureKind::Group {
                children: children.iter().map(self::fixture).collect(),
            },
        },
    }
}

fn point(value: glam::Vec3) -> Point3Meters {
    Point3Meters {
        x_meters: value.x,
        y_meters: value.y,
        z_meters: value.z,
    }
}

fn render_plan(pixels: Vec<SpatialRenderPixel>) -> SpatialRenderPlan {
    let mut bounds = GeometryRenderBounds {
        min_x_meters: 0.0,
        min_y_meters: 0.0,
        max_x_meters: 1.0,
        max_y_meters: 1.0,
    };
    if let Some(first) = pixels.first() {
        bounds.min_x_meters = first.position.x_meters;
        bounds.max_x_meters = first.position.x_meters;
        bounds.min_y_meters = first.position.y_meters;
        bounds.max_y_meters = first.position.y_meters;
        for pixel in &pixels {
            let radius = pixel.diameter_meters / 2.0;
            bounds.min_x_meters = bounds.min_x_meters.min(pixel.position.x_meters - radius);
            bounds.max_x_meters = bounds.max_x_meters.max(pixel.position.x_meters + radius);
            bounds.min_y_meters = bounds.min_y_meters.min(pixel.position.y_meters - radius);
            bounds.max_y_meters = bounds.max_y_meters.max(pixel.position.y_meters + radius);
        }
    }
    SpatialRenderPlan { pixels, bounds }
}
