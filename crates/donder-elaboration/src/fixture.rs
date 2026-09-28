//! Resolve fixture composition and geometry before playback. The runtime receives
//! flat pixel buffers per layout instance, never definition references or groups.

use std::ops::Range;
use std::sync::Arc;

use donder_language::fixture::{
    FixtureDefinition, FixtureDefinitionError, FixtureDefinitionId, FixtureDefinitions,
    FixtureElementId, FixtureGeometryError, FixtureSource, FixtureTransform,
};
use donder_language::layout::{
    FixtureInstanceId, FixtureTarget, Layout, LayoutError, LayoutFixture, LayoutFixtureKind,
    LayoutId,
};
use glam::{Affine3A, EulerRot, Quat, Vec3};
use indexmap::IndexMap;

#[derive(Clone, Debug)]
pub struct PreparedPixel {
    pub element: FixtureElementId,
    /// Stable ordinal within its shape, independent of output reversal.
    pub ordinal: u32,
    /// Meters, relative to the containing definition.
    pub position: Vec3,
    pub diameter_meters: f32,
}

mod shapes;
pub use shapes::{InvalidFixtureElement, element_handles, element_pixels};

#[derive(Clone, Debug, Default)]
pub struct PreparedFixtureDefinitions {
    definitions: IndexMap<FixtureDefinitionId, Arc<[PreparedPixel]>>,
}

#[derive(Clone, Debug)]
pub struct PreparedFixtureInstance {
    pub id: FixtureInstanceId,
    pub pixels: Arc<[PreparedPixel]>,
    pub transform: Affine3A,
}

#[derive(Clone, Debug)]
pub struct PreparedLayout {
    pub id: LayoutId,
    pub instances: Vec<PreparedFixtureInstance>,
    /// Each target selects a contiguous traversal range of whole instances.
    targets: IndexMap<FixtureInstanceId, Range<usize>>,
}

impl PreparedFixtureDefinitions {
    pub fn prepare(definitions: &FixtureDefinitions) -> Result<Self, FixtureDefinitionError> {
        definitions.validate()?;
        Ok(Self {
            definitions: definitions
                .definitions
                .iter()
                .map(|(id, definition)| {
                    let pixels = prepare_geometry(definition).map_err(|error| match error {
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
                    })?;
                    Ok((id.clone(), pixels.into()))
                })
                .collect::<Result<_, FixtureDefinitionError>>()?,
        })
    }

    pub fn pixels(&self, id: &FixtureDefinitionId) -> Option<&[PreparedPixel]> {
        self.definitions.get(id).map(AsRef::as_ref)
    }

    pub fn prepare_layout(&self, layout: &Layout) -> Result<PreparedLayout, LayoutError> {
        layout.validate(&self.definitions)?;
        let mut prepared = PreparedLayout {
            id: layout.id.clone(),
            instances: Vec::new(),
            targets: IndexMap::new(),
        };
        self.prepare_layout_fixtures(&layout.fixtures, &mut prepared)?;
        Ok(prepared)
    }

    fn prepare_layout_fixtures(
        &self,
        fixtures: &[LayoutFixture],
        prepared: &mut PreparedLayout,
    ) -> Result<(), LayoutError> {
        for fixture in fixtures {
            let start = prepared.instances.len();
            match &fixture.kind {
                LayoutFixtureKind::Fixture {
                    definition,
                    transform,
                } => {
                    prepared.instances.push(PreparedFixtureInstance {
                        id: fixture.id,
                        pixels: match definition {
                            FixtureSource::Inline(value) => prepare_geometry(value)
                                .map_err(|error| LayoutError::InvalidFixture {
                                    fixture: fixture.id,
                                    error,
                                })?
                                .into(),
                            FixtureSource::Reference(id) => self
                                .definitions
                                .get(id)
                                .ok_or_else(|| LayoutError::MissingDefinition(id.clone()))?
                                .clone(),
                        },
                        transform: fixture_transform(transform),
                    });
                }
                LayoutFixtureKind::Group { children } => {
                    self.prepare_layout_fixtures(children, prepared)?;
                }
            }
            prepared
                .targets
                .insert(fixture.id, start..prepared.instances.len());
        }
        Ok(())
    }
}

pub fn prepare_geometry(
    definition: &FixtureDefinition,
) -> Result<Vec<PreparedPixel>, FixtureGeometryError> {
    definition.validate_geometry()?;
    let mut pixels = Vec::new();
    for element in &definition.elements {
        pixels.extend(
            element_pixels(element)
                .map_err(|error| FixtureGeometryError::InvalidElement(error.0))?,
        );
    }
    Ok(pixels)
}

impl PreparedLayout {
    pub fn target(
        &self,
        target: &FixtureTarget,
    ) -> Result<&[PreparedFixtureInstance], LayoutError> {
        if target.layout != self.id {
            return Err(LayoutError::WrongLayout(target.layout.clone()));
        }
        let range = self
            .targets
            .get(&target.fixture)
            .ok_or(LayoutError::MissingFixture(target.fixture))?;
        Ok(&self.instances[range.clone()])
    }
}

pub fn fixture_transform(transform: &FixtureTransform) -> Affine3A {
    Affine3A::from_scale_rotation_translation(
        Vec3::new(transform.scale.x, transform.scale.y, transform.scale.z),
        Quat::from_euler(
            EulerRot::XYZ,
            transform.rotation.x.to_radians(),
            transform.rotation.y.to_radians(),
            transform.rotation.z.to_radians(),
        ),
        Vec3::new(
            transform.position.x.as_meters_f32(),
            transform.position.y.as_meters_f32(),
            transform.position.z.as_meters_f32(),
        ),
    )
}
