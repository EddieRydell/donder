use crate::dto::*;
use crate::gui::{ResolvedGuiObject, blocked};
use dawn_elaboration::fixture::PreparedFixtureDefinitions;
use dawn_language::fixture::{FixtureDefinitionId, FixtureTransform};
use dawn_language::layout::{LayoutFixture, LayoutFixtureKind, LayoutId};
use dawn_project_io::{ProjectSession, SourceObjectKind};

pub(in crate::gui) fn project_fixture(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let id = FixtureDefinitionId(resolved.identity.clone());
    let Some(definition) = session.project.definitions.fixtures.definitions.get(&id) else {
        return blocked("Fixture definition was not found.", Vec::new());
    };
    let prepared = match PreparedFixtureDefinitions::prepare(&session.project.definitions.fixtures)
    {
        Ok(prepared) => prepared,
        Err(error) => return blocked(format!("Cannot prepare fixture: {error:?}"), Vec::new()),
    };
    let Some(pixels) = prepared.pixels(&id) else {
        return blocked("Fixture was not prepared.", Vec::new());
    };
    let pixels = pixels
        .iter()
        .enumerate()
        .map(|(index, pixel)| SpatialRenderPixel {
            owner: pixel.id.0,
            index: index as u32,
            position: point(pixel.position),
            diameter_meters: pixel.diameter_meters,
        })
        .collect();
    GuiDocument::Fixture {
        document: FixtureGuiDocument {
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            pixels: definition
                .pixels
                .iter()
                .map(|pixel| GuiPixel {
                    id: pixel.id.0,
                    position: crate::preview::point3_meters(pixel.position),
                    diameter_meters: pixel.diameter.as_meters_f32(),
                })
                .collect(),
            render_plan: render_plan(pixels),
        },
    }
}

pub(in crate::gui) fn project_layout(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let Some(layout) = session
        .project
        .layouts
        .get(&LayoutId(resolved.identity.clone()))
    else {
        return blocked("Layout was not found.", Vec::new());
    };
    let definitions =
        match PreparedFixtureDefinitions::prepare(&session.project.definitions.fixtures) {
            Ok(prepared) => prepared,
            Err(error) => {
                return blocked(format!("Cannot prepare fixtures: {error:?}"), Vec::new());
            }
        };
    let prepared = match definitions.prepare_layout(layout) {
        Ok(prepared) => prepared,
        Err(error) => return blocked(format!("Cannot prepare layout: {error:?}"), Vec::new()),
    };
    let mut pixels = Vec::new();
    for instance in prepared.instances {
        let Some(definition_pixels) = definitions.pixels(&instance.definition) else {
            return blocked("Layout fixture was not prepared.", Vec::new());
        };
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
            available_fixtures: available_fixtures(session),
            render_plan: render_plan(pixels),
        },
    }
}

pub(crate) fn definition_ref(id: &FixtureDefinitionId) -> GuiObjectRef {
    ResolvedGuiObject {
        identity: id.0.clone(),
        kind: SourceObjectKind::FixtureDefinition,
    }
    .source_ref()
}

fn available_fixtures(session: &ProjectSession) -> Vec<GuiObjectRef> {
    session
        .project
        .definitions
        .fixtures
        .definitions
        .keys()
        .map(definition_ref)
        .collect()
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
                definition: definition_ref(definition),
                transform: transform(value),
            },
            LayoutFixtureKind::Group { children } => GuiLayoutFixtureKind::Group {
                children: children.iter().map(self::fixture).collect(),
            },
        },
    }
}

fn transform(value: &FixtureTransform) -> Transform {
    Transform {
        position: crate::preview::point3_meters(value.position),
        rotation: Rotation3Degrees {
            x_degrees: value.rotation.x,
            y_degrees: value.rotation.y,
            z_degrees: value.rotation.z,
        },
        scale: Scale3 {
            x: value.scale.x,
            y: value.scale.y,
            z: value.scale.z,
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
