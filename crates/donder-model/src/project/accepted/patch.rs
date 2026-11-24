//! Runtime output encodings and channel placements admitted with their
//! authored patch and route IDs.
use super::DonderProject;
use crate::patch::{PatchId, PixelRouteId, RoutePlacement};
use crate::validation::{AdmissionProblem, ProjectValidationError, ValidationProblem, at};
use donder_runtime_types::OutputEncoding;
use indexmap::IndexMap;
use std::fmt;

pub(super) struct AdmittedPatch {
    pub(super) routes: IndexMap<PixelRouteId, PlaybackRoute>,
}

/// A route as playback sends it.
pub struct PlaybackRoute {
    pub encoding: OutputEncoding,
    pub placement: RoutePlacement,
}

impl fmt::Debug for AdmittedPatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdmittedPatch")
            .field("route_count", &self.routes.len())
            .finish()
    }
}

pub(super) fn admit(
    project: &DonderProject,
) -> Result<IndexMap<PatchId, AdmittedPatch>, ProjectValidationError> {
    let counts = project.fixture_pixel_counts();
    project
        .objects::<crate::Patch>()
        .map(|patch| {
            let mut placements = crate::patch::patch_placements(project, patch, &counts);
            let routes = patch
                .routes
                .iter()
                .map(|route| {
                    let Some(encoding) = OutputEncoding::admit(route.encoding) else {
                        unreachable!("validation proved every route's color order");
                    };
                    let placement =
                        placements
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
                .map_err(at(patch.id))?;
            Ok((patch.id, AdmittedPatch { routes }))
        })
        .collect()
}
