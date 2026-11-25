//! Runtime output encodings and channel placements admitted with their
//! authored route IDs.
use super::DonderProject;
use crate::patch::{PixelRouteId, RoutePlacement};
use crate::validation::{AdmissionProblem, ProjectValidationError, ValidationProblem, at};
use donder_runtime_types::OutputEncoding;
use indexmap::IndexMap;
use std::fmt;

#[derive(Default)]
pub(super) struct AdmittedRoutes {
    pub(super) routes: IndexMap<PixelRouteId, PlaybackRoute>,
}

/// A route as playback sends it.
pub struct PlaybackRoute {
    pub encoding: OutputEncoding,
    pub placement: RoutePlacement,
}

impl fmt::Debug for AdmittedRoutes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdmittedRoutes")
            .field("route_count", &self.routes.len())
            .finish()
    }
}

pub(super) fn admit(project: &DonderProject) -> Result<AdmittedRoutes, ProjectValidationError> {
    let counts = project.fixture_pixel_counts();
    let root = project.root();
    let mut placements = crate::patch::route_placements(project, &root.routes, &counts);
    let routes = root
        .routes
        .iter()
        .map(|route| {
            let Some(encoding) = OutputEncoding::admit(route.encoding) else {
                unreachable!("validation proved every route's color order");
            };
            let placement = placements
                .remove(&route.id)
                .ok_or(ValidationProblem::Admission(
                    AdmissionProblem::UnplacedRoute(route.id),
                ))?;
            Ok((
                route.id,
                PlaybackRoute {
                    encoding,
                    placement,
                },
            ))
        })
        .collect::<Result<_, ValidationProblem>>()
        .map_err(at(root.id))?;
    Ok(AdmittedRoutes { routes })
}
