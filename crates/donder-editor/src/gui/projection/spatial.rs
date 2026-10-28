use crate::dto::{
    FixtureGuiDocument, GeometryRenderBounds, GuiDocument, GuiFixtureHandle, GuiLayoutFixture,
    GuiLayoutFixtureKind, GuiObjectRef, GuiOwnedFixture, LayoutGuiDocument, OwnedObjectSlot,
    Point3Meters, SpatialRenderPixel, SpatialRenderPlan,
};
use crate::gui::{ResolvedGuiObject, blocked};
use donder_model::FixtureDefinitionId;
use donder_model::PreparedFixtureDefinitions;
use donder_model::{LayoutFixture, LayoutFixtureKind, LayoutId};
use donder_project_io::ProjectSession;

pub(in crate::gui) fn project_fixture(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let (definition, name) = match resolved.owned_path.as_slice() {
        [] => {
            let Some(value) = session
                .project
                .reusable::<donder_model::FixtureDefinition>()
                .get(&resolved.id::<FixtureDefinitionId>())
            else {
                return blocked("Fixture was not found.", Vec::new());
            };
            (value, resolved.identity.object().to_string())
        }
        [.., OwnedObjectSlot::Fixture { id }] => {
            let Some(placement) = session
                .project
                .get::<donder_model::Layout>(resolved.id::<LayoutId>())
                .and_then(|layout| layout.fixture(donder_model::FixtureInstanceId(*id)))
            else {
                return blocked("Fixture was not found.", Vec::new());
            };
            let LayoutFixtureKind::Fixture {
                definition: donder_model::OwnedFixture::Inline(value),
                ..
            } = &placement.kind
            else {
                return blocked("Fixture is not owned inline.", Vec::new());
            };
            (value, placement.name.as_str().to_string())
        }
        _ => return blocked("Unsupported owned fixture path.", Vec::new()),
    };
    let pixels = donder_model::prepare_geometry(definition);
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
            donder_model::element_handles(element)
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
            description: definition.description.clone(),
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
        .get::<donder_model::Layout>(resolved.id::<LayoutId>())
    else {
        return blocked("Layout was not found.", Vec::new());
    };
    let definitions = PreparedFixtureDefinitions::prepare(
        session
            .project
            .reusable::<donder_model::FixtureDefinition>(),
    );
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
            description: layout.description.clone(),
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            fixtures: layout
                .fixtures
                .iter()
                .map(|value| fixture(session, value))
                .collect(),
            root: layout.root.iter().map(|member| member.0).collect(),
            available_fixtures: crate::gui::ownership::available_declarations(
                session,
                &[donder_model::ObjectKind::FixtureDefinition],
            ),
            render_plan: render_plan(pixels),
        },
    }
}

pub(crate) fn definition_ref(session: &ProjectSession, id: &FixtureDefinitionId) -> GuiObjectRef {
    crate::gui::model::object_ref(session, *id)
}

fn fixture(session: &ProjectSession, fixture: &LayoutFixture) -> GuiLayoutFixture {
    GuiLayoutFixture {
        id: fixture.id.0,
        name: fixture.name.as_str().to_string(),
        description: fixture.description.clone(),
        kind: match &fixture.kind {
            LayoutFixtureKind::Fixture {
                definition,
                transform: value,
            } => GuiLayoutFixtureKind::Fixture {
                definition: match definition {
                    donder_model::OwnedFixture::Inline(value) => GuiOwnedFixture::Inline {
                        elements: value
                            .elements
                            .iter()
                            .map(crate::gui::fixture::gui_element)
                            .collect(),
                    },
                    donder_model::OwnedFixture::Reference(id) => GuiOwnedFixture::Reference {
                        object: definition_ref(session, id),
                    },
                },
                transform: crate::gui::fixture::gui_transform(value),
            },
            LayoutFixtureKind::Group { members } => GuiLayoutFixtureKind::Group {
                members: members.iter().map(|member| member.0).collect(),
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
