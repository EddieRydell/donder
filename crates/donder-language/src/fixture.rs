//! Reusable fixtures contain ordered pixels. Layouts own grouping and placement.
use crate::identity::SourceIdentity;
use crate::values::{DistanceSpan, Point3, Rotation3, Scale3};
use indexmap::IndexMap;
use std::collections::HashSet;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FixtureDefinitionId(pub SourceIdentity);

/// Stable within a definition; list order determines output order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PixelId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Pixel {
    pub id: PixelId,
    pub position: Point3,
    pub diameter: DistanceSpan,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FixtureTransform {
    pub position: Point3,
    pub rotation: Rotation3,
    pub scale: Scale3,
}
impl FixtureTransform {
    pub fn is_valid(&self) -> bool {
        [self.rotation.x, self.rotation.y, self.rotation.z]
            .into_iter()
            .all(f32::is_finite)
            && [self.scale.x, self.scale.y, self.scale.z]
                .into_iter()
                .all(|value| value.is_finite() && value != 0.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureDefinition {
    pub pixels: Vec<Pixel>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FixtureDefinitions {
    pub definitions: IndexMap<FixtureDefinitionId, FixtureDefinition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FixtureDefinitionError {
    TooManyPixels(FixtureDefinitionId),
    DuplicatePixel {
        definition: FixtureDefinitionId,
        pixel: PixelId,
    },
    EmptyPixel {
        definition: FixtureDefinitionId,
        pixel: PixelId,
    },
}
impl FixtureDefinitions {
    pub fn pixel_counts(
        &self,
    ) -> Result<IndexMap<FixtureDefinitionId, u32>, FixtureDefinitionError> {
        self.validate()?;
        self.definitions
            .iter()
            .map(|(id, definition)| {
                u32::try_from(definition.pixels.len())
                    .map(|count| (id.clone(), count))
                    .map_err(|_| FixtureDefinitionError::TooManyPixels(id.clone()))
            })
            .collect()
    }
    pub fn validate(&self) -> Result<(), FixtureDefinitionError> {
        for (id, definition) in &self.definitions {
            let mut seen = HashSet::new();
            for pixel in &definition.pixels {
                if !seen.insert(pixel.id) {
                    return Err(FixtureDefinitionError::DuplicatePixel {
                        definition: id.clone(),
                        pixel: pixel.id,
                    });
                }
                if pixel.diameter == DistanceSpan::ZERO {
                    return Err(FixtureDefinitionError::EmptyPixel {
                        definition: id.clone(),
                        pixel: pixel.id,
                    });
                }
            }
        }
        Ok(())
    }
}
