//! Reusable fixtures contain ordered shapes. Layouts own grouping and placement.
use crate::identity::SourceIdentity;
use donder_language::{DistanceSpan, Point3, Rotation3, Scale3};
use indexmap::IndexMap;
use std::collections::HashSet;

pub(crate) const MAX_FIXTURE_PIXELS: u32 = 1_000_000;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FixtureDefinitionId(pub SourceIdentity);

/// Stable within a definition; list order determines output order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FixtureElementId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureElement {
    pub id: FixtureElementId,
    /// Unique within the fixture definition.
    pub name: donder_runtime_types::Identifier,
    pub transform: FixtureTransform,
    pub diameter: DistanceSpan,
    pub reverse: bool,
    pub shape: FixtureShape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridAxis {
    Rows,
    Columns,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridCorner {
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FixtureShape {
    Pixel,
    Line {
        length: f32,
        count: u32,
    },
    Polyline {
        points: Vec<Point3>,
        count: u32,
    },
    Arc {
        radius: f32,
        start_degrees: f32,
        sweep_degrees: f32,
        count: u32,
        closed: bool,
    },
    Grid {
        columns: u32,
        rows: u32,
        width: f32,
        height: f32,
        axis: GridAxis,
        corner: GridCorner,
        serpentine: bool,
    },
}

impl FixtureShape {
    pub fn pixel_count(&self) -> Option<u32> {
        match self {
            Self::Pixel => Some(1),
            Self::Line { count, .. } | Self::Polyline { count, .. } | Self::Arc { count, .. } => {
                Some(*count)
            }
            Self::Grid { columns, rows, .. } => columns.checked_mul(*rows),
        }
    }

    pub fn is_valid(&self) -> bool {
        let positive = |v: f32| v.is_finite() && v > 0.0 && v <= 2_000.0;
        if !matches!(self.pixel_count(), Some(1..=MAX_FIXTURE_PIXELS)) {
            return false;
        }
        match self {
            Self::Pixel => true,
            Self::Line { length, .. } => positive(*length),
            Self::Polyline { points, .. } => {
                points.len() >= 2 && points.windows(2).any(|pair| pair[0] != pair[1])
            }
            Self::Arc {
                radius,
                start_degrees,
                sweep_degrees,
                closed,
                ..
            } => {
                positive(*radius)
                    && start_degrees.is_finite()
                    && sweep_degrees.is_finite()
                    && *sweep_degrees != 0.0
                    && sweep_degrees.abs() <= 360.0
                    && (!closed || sweep_degrees.abs() == 360.0)
            }
            Self::Grid {
                columns,
                rows,
                width,
                height,
                ..
            } => *columns > 0 && *rows > 0 && positive(*width) && positive(*height),
        }
    }
}

impl FixtureElement {
    pub fn is_valid(&self) -> bool {
        donder_language::NameKind::Object.accepts(self.name.as_str())
            && self.transform.is_valid()
            && self.diameter != DistanceSpan::ZERO
            && self.diameter.as_meters_f32() <= 100.0
            && self.shape.is_valid()
    }
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
    pub description: Option<String>,
    pub elements: Vec<FixtureElement>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FixtureDefinitions {
    pub definitions: IndexMap<FixtureDefinitionId, FixtureDefinition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FixtureDefinitionError {
    TooManyPixels(FixtureDefinitionId),
    DuplicateElement {
        definition: FixtureDefinitionId,
        element: FixtureElementId,
    },
    InvalidElement {
        definition: FixtureDefinitionId,
        element: FixtureElementId,
    },
}
pub type FixtureSource = crate::ownership::ValueSource<FixtureDefinition, FixtureDefinitionId>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FixtureGeometryError {
    TooManyPixels,
    DuplicateElement(FixtureElementId),
    InvalidElement(FixtureElementId),
}

impl FixtureDefinition {
    pub fn validate_geometry(&self) -> Result<u32, FixtureGeometryError> {
        let mut seen = HashSet::new();
        let mut names = HashSet::new();
        let mut total = 0u32;
        for element in &self.elements {
            if !seen.insert(element.id) || !names.insert(&element.name) {
                return Err(FixtureGeometryError::DuplicateElement(element.id));
            }
            if !element.is_valid() {
                return Err(FixtureGeometryError::InvalidElement(element.id));
            }
            total = total
                .checked_add(
                    element
                        .shape
                        .pixel_count()
                        .ok_or(FixtureGeometryError::TooManyPixels)?,
                )
                .filter(|count| *count <= MAX_FIXTURE_PIXELS)
                .ok_or(FixtureGeometryError::TooManyPixels)?;
            // Derived coordinates must be valid at authoring acceptance, not
            // discovered later while preparing playback or drawing a preview.
            crate::geometry::element_pixels(element)
                .map_err(|_| FixtureGeometryError::InvalidElement(element.id))?;
        }
        Ok(total)
    }

    pub fn validate(&self, id: &FixtureDefinitionId) -> Result<u32, FixtureDefinitionError> {
        self.validate_geometry().map_err(|error| match error {
            FixtureGeometryError::TooManyPixels => {
                FixtureDefinitionError::TooManyPixels(id.clone())
            }
            FixtureGeometryError::DuplicateElement(element) => {
                FixtureDefinitionError::DuplicateElement {
                    definition: id.clone(),
                    element,
                }
            }
            FixtureGeometryError::InvalidElement(element) => {
                FixtureDefinitionError::InvalidElement {
                    definition: id.clone(),
                    element,
                }
            }
        })
    }
}

impl FixtureDefinitions {
    pub fn pixel_counts(
        &self,
    ) -> Result<IndexMap<FixtureDefinitionId, u32>, FixtureDefinitionError> {
        self.definitions
            .iter()
            .map(|(id, definition)| definition.validate(id).map(|count| (id.clone(), count)))
            .collect()
    }
    pub fn validate(&self) -> Result<(), FixtureDefinitionError> {
        for (id, definition) in &self.definitions {
            definition.validate(id)?;
        }
        Ok(())
    }
}
