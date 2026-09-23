use crate::dto::*;
use crate::gui::model::source_identity_from_gui;
use crate::gui::{GuiMutationError, ResolvedGuiObject, blocked};
use donder_language::controller::{ControllerId, ControllerPortId};
use donder_language::identity::SourceIdentity;
use donder_language::layout::{FixtureInstanceId, FixtureTarget as DomainFixtureTarget, LayoutId};
use donder_language::patch::{PatchId, PixelEncoding, PixelRoute, PixelRouteId, PixelSpan};
use donder_project_io::{ProjectSession, SourceObjectKind, ensure_document_can_reference_source};

pub(super) fn project_document(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let Some(patch) = session
        .project
        .patches
        .get(&PatchId(resolved.identity.clone()))
    else {
        return blocked("Patch was not found.", Vec::new());
    };
    let counts = match session.project.definitions.fixtures.pixel_counts() {
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
        .layouts
        .iter()
        .map(|(id, layout)| {
            let fixtures = layout
                .iter_fixtures()
                .map(|fixture| {
                    Ok(PatchFixtureTarget {
                        id: fixture.id.0,
                        name: fixture.name.clone(),
                        pixel_count: layout.target_pixel_count(
                            &DomainFixtureTarget {
                                layout: id.clone(),
                                fixture: fixture.id,
                            },
                            &counts,
                        )?,
                    })
                })
                .collect::<Result<_, donder_language::layout::LayoutError>>()?;
            Ok(PatchLayout {
                source_ref: object_ref(&id.0, SourceObjectKind::Layout),
                fixtures,
            })
        })
        .collect::<Result<_, donder_language::layout::LayoutError>>();
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
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            routes: patch.routes.iter().map(project_route).collect(),
            layouts,
            controllers: session
                .project
                .controllers
                .iter()
                .map(|(id, controller)| {
                    super::controller::project_controller(session, id, controller)
                })
                .collect(),
        },
    }
}

pub(super) fn object_ref(identity: &SourceIdentity, kind: SourceObjectKind) -> GuiObjectRef {
    ResolvedGuiObject {
        identity: identity.clone(),
        kind,
    }
    .source_ref()
}

fn project_route(route: &PixelRoute) -> GuiPixelRoute {
    GuiPixelRoute {
        id: route.id.0,
        layout: object_ref(&route.target.layout.0, SourceObjectKind::Layout),
        fixture: route.target.fixture.0,
        pixels: route.pixels.map(|pixels| GuiPixelSpan {
            start: pixels.start,
            count: pixels.count,
        }),
        controller: object_ref(&route.controller.0, SourceObjectKind::Controller),
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
            let layout = source_identity_from_gui(
                &route.layout.module_id,
                &route.layout.path,
                &route.layout.object_key,
            )?;
            let controller = source_identity_from_gui(
                &route.controller.module_id,
                &route.controller.path,
                &route.controller.object_key,
            )?;
            for (kind, identity) in [
                (SourceObjectKind::Layout, &layout),
                (SourceObjectKind::Controller, &controller),
            ] {
                ensure_document_can_reference_source(session, id.0.document_id(), kind, identity)
                    .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
            }
            Ok(PixelRoute {
                id: PixelRouteId(route.id),
                target: DomainFixtureTarget {
                    layout: LayoutId(layout),
                    fixture: FixtureInstanceId(route.fixture),
                },
                pixels: route.pixels.map(|pixels| PixelSpan {
                    start: pixels.start,
                    count: pixels.count,
                }),
                controller: ControllerId(controller),
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
        .patches
        .get_mut(id)
        .ok_or_else(|| GuiMutationError::Invalid("Patch was not found.".into()))?
        .routes = routes;
    Ok(())
}
