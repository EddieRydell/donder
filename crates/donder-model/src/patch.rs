//! LED output routing. Pixel ranges describe physical wiring, not effect targets.
use std::collections::HashMap;
use std::ops::Range;

use donder_runtime_types::PixelEncoding;
use indexmap::IndexMap;

use crate::controller::{Controller, ControllerId, ControllerOutput, ControllerPortId};
use crate::fixture::FixtureDefinitionId;
use crate::project::DonderProject;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PixelRouteId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PixelSpan {
    pub start: u32,
    pub count: u32,
}

/// Where a route's channels begin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteStart {
    /// Right after the route before it on its port, in route order. The
    /// first route on an E1.31 or Art-Net port follows the last route on the
    /// controller's previous port; on a Donder port it starts the port's
    /// stream.
    Next,
    /// A channel from 0: across all of an E1.31 or Art-Net controller's
    /// universes, or in a Donder port.
    Channel(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PixelRoute {
    pub id: PixelRouteId,
    pub target: crate::layout::FixtureId,
    /// None routes the complete target, including future pixel-count edits.
    pub pixels: Option<PixelSpan>,
    /// Sends its pixels last first, for wiring that runs against the
    /// fixture's pixel order, such as the second half of a strand folded back.
    pub reverse: bool,
    pub controller: ControllerId,
    pub port: ControllerPortId,
    pub start: RouteStart,
    pub encoding: PixelEncoding,
    pub gamma: f32,
    pub brightness: f32,
    /// Vary each byte over frames so it averages to the exact level, for
    /// outputs that receive every frame.
    pub dither: bool,
}

impl PixelRoute {
    /// The channels it sends when its target has `target_pixels` pixels.
    pub fn channel_count(&self, target_pixels: u32) -> Option<u32> {
        self.pixels
            .map_or(target_pixels, |span| span.count)
            .checked_mul(self.encoding.channel_count() as u32)
    }
}

/// The controller stream a route plays in and the channels it takes there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutePlacement {
    pub output: ControllerOutput,
    pub channels: Range<u32>,
}

/// Each route's channels, resolving `next` starts through each controller's
/// ports in order and each port's routes in route order. `width` is a
/// route's channel count. A route on a missing controller or port, whose
/// width is unknown, whose channels overflow, or that follows such a route,
/// has no placement.
pub(crate) fn place_routes<'a>(
    routes: &[PixelRoute],
    controller: impl Fn(ControllerId) -> Option<&'a Controller>,
    width: impl Fn(&PixelRoute) -> Option<u32>,
) -> HashMap<PixelRouteId, RoutePlacement> {
    let mut by_controller: IndexMap<ControllerId, Vec<&PixelRoute>> = IndexMap::new();
    for route in routes {
        by_controller
            .entry(route.controller)
            .or_default()
            .push(route);
    }
    let mut placements = HashMap::new();
    for (id, routes) in by_controller {
        let Some(controller) = controller(id) else {
            continue;
        };
        let shared = controller.universes().is_some();
        let mut cursor = Some(0);
        for port in controller.ports() {
            let output = controller.own_port_output(port.id);
            if !shared {
                cursor = Some(0);
            }
            for route in routes.iter().filter(|route| route.port == port.id) {
                let start = match route.start {
                    RouteStart::Next => cursor,
                    RouteStart::Channel(channel) => Some(channel),
                };
                cursor = start
                    .zip(width(route))
                    .and_then(|(start, width)| start.checked_add(width));
                if let (Some(start), Some(end)) = (start, cursor) {
                    placements.insert(
                        route.id,
                        RoutePlacement {
                            output,
                            channels: start..end,
                        },
                    );
                }
            }
        }
    }
    placements
}

/// `place_routes` for routes of `project`, with each route's width from its
/// target's pixels.
pub fn route_placements(
    project: &DonderProject,
    routes: &[PixelRoute],
    counts: &IndexMap<FixtureDefinitionId, u32>,
) -> HashMap<PixelRouteId, RoutePlacement> {
    place_routes(
        routes,
        |id| project.get::<Controller>(id),
        |route| {
            let pixels = project
                .layout()
                .target_pixel_count(route.target, counts)
                .ok()?;
            route.channel_count(pixels)
        },
    )
}

/// A channel's output level from its color level, both from 0 to 1: the
/// route's gamma, then its brightness.
pub fn patched_level(level: f32, gamma: f32, brightness: f32) -> f32 {
    level.powf(gamma) * brightness
}
