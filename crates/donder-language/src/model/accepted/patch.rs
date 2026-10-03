//! Runtime output encodings admitted with their authored patch and route IDs.
use super::DonderProject;
use crate::execution::OutputEncoding;
use crate::patch::{PatchId, PixelRouteId};
use crate::validation::ProjectValidationError;
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
        .patches()
        .map(|patch| {
            let routes = patch
                .routes
                .iter()
                .map(|route| {
                    let encoding = OutputEncoding::admit(route.encoding).ok_or_else(|| {
                        ProjectValidationError::InvalidRelationship(format!(
                            "Invalid output encoding for route {} in patch {:?}",
                            route.id.0, patch.id
                        ))
                    })?;
                    Ok((route.id, encoding))
                })
                .collect::<Result<_, ProjectValidationError>>()?;
            Ok((patch.id.clone(), PatchEncodings { routes }))
        })
        .collect()
}
