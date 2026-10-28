//! Runtime output encodings admitted with their authored patch and route IDs.
use super::DonderProject;
use crate::patch::{PatchId, PixelRouteId};
use crate::validation::{AdmissionProblem, ProjectValidationError, ValidationProblem, at};
use donder_runtime_types::OutputEncoding;
use indexmap::IndexMap;
use std::fmt;

pub(super) struct PatchEncodings {
    pub(super) routes: IndexMap<PixelRouteId, OutputEncoding>,
}

impl fmt::Debug for PatchEncodings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PatchEncodings")
            .field("route_count", &self.routes.len())
            .finish()
    }
}

pub(super) fn admit(
    project: &DonderProject,
) -> Result<IndexMap<PatchId, PatchEncodings>, ProjectValidationError> {
    project
        .objects::<crate::Patch>()
        .map(|patch| {
            let routes = patch
                .routes
                .iter()
                .map(|route| {
                    let encoding = OutputEncoding::admit(route.encoding).ok_or(
                        ValidationProblem::Admission(AdmissionProblem::InvalidEncoding(route.id)),
                    )?;
                    Ok((route.id, encoding))
                })
                .collect::<Result<_, ValidationProblem>>()
                .map_err(at(patch.id))?;
            Ok((patch.id, PatchEncodings { routes }))
        })
        .collect()
}
