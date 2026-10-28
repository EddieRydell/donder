use crate::dto::{
    GuiDocument, GuiPixelEncoding, GuiPixelRoute, GuiPixelSpan, ObjectKind, PatchFixtureTarget,
    PatchGuiDocument, PatchLayout,
};
use crate::gui::model::{object_address, object_id_from_gui, object_ref};
use crate::gui::{GuiMutationError, ResolvedGuiObject, blocked};
use donder_model::{ControllerId, ControllerPortId};
use donder_model::{FixtureInstanceId, FixtureTarget as DomainFixtureTarget, LayoutId};
use donder_model::{PatchId, PixelRoute, PixelRouteId, PixelSpan};
use donder_project_io::{ProjectSession, ensure_document_can_reference_object};
use donder_runtime_types::PixelEncoding;

pub(super) fn project_document(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let Some(patch) = session
        .project
        .get::<donder_model::Patch>(resolved.id::<PatchId>())
    else {
        return blocked("Patch was not found.", Vec::new());
    };
    let counts = match donder_model::fixture_pixel_counts(
        session
            .project
            .reusable::<donder_model::FixtureDefinition>(),
    ) {
        Ok(counts) => counts,
        Err(error) => {
            return blocked(
                format!("Cannot count fixture pixels: {error:?}"),
                Vec::new(),
            );
        }
    };
    let layouts = session
        .project
        .objects::<donder_model::Layout>()
        .map(|layout| {
            let id = &layout.id;
            // Routes target fixtures; groups are effect targets only.
            let fixtures = layout
                .iter_fixtures()
                .filter(|fixture| {
                    matches!(
                        fixture.kind,
                        donder_model::LayoutFixtureKind::Fixture { .. }
                    )
                })
                .map(|fixture| {
                    Ok(PatchFixtureTarget {
                        id: fixture.id.0,
                        name: fixture.name.as_str().to_string(),
                        pixel_count: layout.target_pixel_count(
                            &DomainFixtureTarget {
                                layout: *id,
                                fixture: fixture.id,
                            },
                            &counts,
                        )?,
                    })
                })
                .collect::<Result<_, donder_model::LayoutError>>()?;
            Ok(PatchLayout {
                source_ref: object_ref(session, *id),
                fixtures,
            })
        })
        .collect::<Result<_, donder_model::LayoutError>>();
    let layouts = match layouts {
        Ok(layouts) => layouts,
        Err(error) => {
            return blocked(
                format!("Cannot resolve patch targets: {error:?}"),
                Vec::new(),
            );
        }
    };
    GuiDocument::Patch {
        document: PatchGuiDocument {
            description: patch.description.clone(),
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            routes: patch
                .routes
                .iter()
                .map(|route| project_route(session, route))
                .collect(),
            layouts,
            controllers: session
                .project
                .objects::<donder_model::Controller>()
                .map(|controller| {
                    let id = &controller.id;
                    super::controller::project_controller(session, id, controller)
                })
                .collect(),
        },
    }
}

fn project_route(session: &ProjectSession, route: &PixelRoute) -> GuiPixelRoute {
    GuiPixelRoute {
        id: route.id.0,
        layout: object_ref(session, route.target.layout),
        fixture: route.target.fixture.0,
        pixels: route.pixels.map(|pixels| GuiPixelSpan {
            start: pixels.start,
            count: pixels.count,
        }),
        controller: object_ref(session, route.controller),
        port: route.port.0,
        start_slot: route.start_slot,
        encoding: match route.encoding {
            PixelEncoding::Rgb { order } => GuiPixelEncoding::Rgb { order },
            PixelEncoding::Rgbw { order } => GuiPixelEncoding::Rgbw { order },
        },
        gamma: route.gamma,
        brightness: route.brightness,
    }
}

pub(super) fn replace(
    session: &mut ProjectSession,
    id: &PatchId,
    routes: Vec<GuiPixelRoute>,
) -> Result<(), GuiMutationError> {
    let document = object_address(session, *id).document_id().clone();
    let routes = routes
        .into_iter()
        .map(|route| {
            if !matches!(route.layout.kind, ObjectKind::Layout)
                || !matches!(route.controller.kind, ObjectKind::Controller)
            {
                return Err(GuiMutationError::Invalid(
                    "A pixel route needs a layout and a controller.".into(),
                ));
            }
            let layout: LayoutId = object_id_from_gui(session, &route.layout)?;
            let controller: ControllerId = object_id_from_gui(session, &route.controller)?;
            for target in [layout.into(), controller.into()] {
                ensure_document_can_reference_object(session, &document, target)
                    .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
            }
            Ok(PixelRoute {
                id: PixelRouteId(route.id),
                target: DomainFixtureTarget {
                    layout,
                    fixture: FixtureInstanceId(route.fixture),
                },
                pixels: route.pixels.map(|pixels| PixelSpan {
                    start: pixels.start,
                    count: pixels.count,
                }),
                controller,
                port: ControllerPortId(route.port),
                start_slot: route.start_slot,
                encoding: match route.encoding {
                    GuiPixelEncoding::Rgb { order } => PixelEncoding::Rgb { order },
                    GuiPixelEncoding::Rgbw { order } => PixelEncoding::Rgbw { order },
                },
                gamma: route.gamma,
                brightness: route.brightness,
            })
        })
        .collect::<Result<_, _>>()?;
    session
        .project
        .put(donder_model::Patch {
            id: *id,
            description: session
                .project
                .get::<donder_model::Patch>(*id)
                .and_then(|patch| patch.description.clone()),
            routes,
        })
        .map_err(GuiMutationError::from)
}
