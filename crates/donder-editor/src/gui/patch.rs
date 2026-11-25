use crate::dto::{
    GuiChannelPlace, GuiPixelEncoding, GuiPixelRoute, GuiPixelSpan, GuiRoutePlacement,
    GuiRouteStart, ObjectKind, PatchFixtureTarget, PatchGuiDocument,
};
use crate::gui::GuiMutationError;
use crate::gui::model::{object_id_from_gui, object_ref};
use donder_model::FixtureId;
use donder_model::{ChannelPlace, ControllerId, ControllerPortId, RouteStart};
use donder_model::{PixelRoute, PixelRouteId, PixelSpan};
use donder_project_io::ProjectSession;
use donder_runtime_types::PixelEncoding;

/// The outputs editor, or why the project's fixtures can't be found.
pub(super) fn project_outputs(session: &ProjectSession) -> Result<PatchGuiDocument, String> {
    let routes = &session.project().root().routes;
    let counts = session.project().fixture_pixel_counts();
    let layout = session.project().layout();
    // Routes target fixtures; groups are clip targets only.
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
                pixel_count: layout.target_pixel_count(fixture.id, &counts)?,
            })
        })
        .collect::<Result<_, donder_model::LayoutError>>();
    let fixtures =
        fixtures.map_err(|error| format!("The project's fixtures can't be found: {error}."))?;
    Ok(PatchGuiDocument {
        routes: routes
            .iter()
            .map(|route| project_route(session, route))
            .collect(),
        placements: {
            let placed = donder_model::route_placements(session.project(), routes, &counts);
            routes
                .iter()
                .filter_map(|route| placement(session, route, placed.get(&route.id)?))
                .collect()
        },
        fixtures,
        controllers: session
            .project()
            .objects::<donder_model::Controller>()
            .map(|(controller_id, controller)| {
                let id = &controller_id;
                super::controller::project_controller(session, id, controller)
            })
            .collect(),
    })
}

/// Where a route's `placed` channels land on its controller, as people count.
fn placement(
    session: &ProjectSession,
    route: &PixelRoute,
    placed: &donder_model::RoutePlacement,
) -> Option<GuiRoutePlacement> {
    let place = session
        .project()
        .get::<donder_model::Controller>(route.controller)?
        .place(placed.output, placed.channels.clone())?;
    Some(GuiRoutePlacement {
        route: route.id.0,
        place: match place {
            ChannelPlace::Universes { first, last } => GuiChannelPlace::Universes {
                first_universe: first.universe,
                first_channel: first.channel,
                last_universe: last.universe,
                last_channel: last.channel,
            },
            ChannelPlace::Port { port, first, last } => GuiChannelPlace::Port { port, first, last },
        },
    })
}

fn project_route(session: &ProjectSession, route: &PixelRoute) -> GuiPixelRoute {
    GuiPixelRoute {
        id: route.id.0,
        fixture: route.target.0,
        pixels: route.pixels.map(|pixels| GuiPixelSpan {
            start: pixels.start,
            count: pixels.count,
        }),
        reverse: route.reverse,
        controller: object_ref(session, route.controller),
        port: route.port.0,
        start: match route.start {
            RouteStart::Next => GuiRouteStart::Next,
            RouteStart::Channel(channel) => GuiRouteStart::Channel { channel },
        },
        encoding: match route.encoding {
            PixelEncoding::Rgb { order } => GuiPixelEncoding::Rgb { order },
            PixelEncoding::Rgbw { order } => GuiPixelEncoding::Rgbw { order },
            PixelEncoding::Channels { colors } => GuiPixelEncoding::Channels {
                colors: colors
                    .as_slice()
                    .iter()
                    .map(|bytes| {
                        let [red, green, blue] = bytes.map(|part| f32::from(part) / 255.0);
                        donder_runtime_types::Color { red, green, blue }.to_hex()
                    })
                    .collect(),
            },
        },
        gamma: route.gamma,
        brightness: route.brightness,
        dither: route.dither,
    }
}

pub(super) fn replace(
    session: &mut ProjectSession,
    routes: Vec<GuiPixelRoute>,
) -> Result<(), GuiMutationError> {
    let routes = routes
        .into_iter()
        .map(|route| {
            if !matches!(route.controller.kind, ObjectKind::Controller) {
                return Err(GuiMutationError::Invalid(
                    "A pixel route needs a controller.".into(),
                ));
            }
            let controller: ControllerId = object_id_from_gui(session, &route.controller)?;
            Ok(PixelRoute {
                id: PixelRouteId(route.id),
                target: FixtureId(route.fixture),
                pixels: route.pixels.map(|pixels| PixelSpan {
                    start: pixels.start,
                    count: pixels.count,
                }),
                reverse: route.reverse,
                controller,
                port: ControllerPortId(route.port),
                start: match route.start {
                    GuiRouteStart::Next => RouteStart::Next,
                    GuiRouteStart::Channel { channel } => RouteStart::Channel(channel),
                },
                encoding: match route.encoding {
                    GuiPixelEncoding::Rgb { order } => PixelEncoding::Rgb { order },
                    GuiPixelEncoding::Rgbw { order } => PixelEncoding::Rgbw { order },
                    GuiPixelEncoding::Channels { colors } => {
                        let bytes = colors
                            .iter()
                            .map(|color| {
                                crate::gui::model::parse_color(color).map(|color| {
                                    [color.red, color.green, color.blue]
                                        .map(|part| (part.clamp(0.0, 1.0) * 255.0).round() as u8)
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        PixelEncoding::Channels {
                            colors: donder_runtime_types::ChannelColors::new(&bytes).ok_or_else(
                                || {
                                    GuiMutationError::Invalid(
                                        "An output's channel colors are 1 to 4 colors, none black."
                                            .into(),
                                    )
                                },
                            )?,
                        }
                    }
                },
                gamma: route.gamma,
                brightness: route.brightness,
                dither: route.dither,
            })
        })
        .collect::<Result<_, _>>()?;
    let mut root = session.project().root().clone();
    root.routes = routes;
    session.put(root).map_err(GuiMutationError::from)
}
