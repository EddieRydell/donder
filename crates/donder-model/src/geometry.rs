//! Shared fixture geometry for authoring previews and sequence preparation.
//!
//! Expands shapes and layout transforms without compiling effects or creating
//! playback state. Both the editor and elaboration use this same geometry.

use std::sync::Arc;

use crate::fixture::{FixtureDefinition, FixtureDefinitionId, FixtureElementId, FixtureTransform};
use crate::layout::{FixtureId, Layout, LayoutError, LayoutFixtureKind};
use glam::{Affine3A, EulerRot, Quat, Vec3};
use indexmap::IndexMap;

#[derive(Clone, Debug)]
pub struct PreparedPixel {
    pub element: FixtureElementId,
    /// Stable ordinal within its shape, independent of output reversal.
    pub ordinal: u32,
    /// The strand of its fixture it lies on: each element is one, and a
    /// grid's rows or columns, as it is wired, are one each.
    pub strand: u32,
    /// Its place along its strand, in wiring order.
    pub along: u32,
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
    pub id: FixtureId,
    pub pixels: Arc<[PreparedPixel]>,
    pub transform: Affine3A,
}

#[derive(Clone, Debug)]
pub struct PreparedLayout {
    pub instances: Vec<PreparedFixtureInstance>,
    /// Each target's placed instances in target order, each once, whether
    /// it selects all of an instance's pixels or a slice.
    targets: IndexMap<FixtureId, Vec<usize>>,
}

impl PreparedFixtureDefinitions {
    /// Expand definitions from an accepted project. Validation belongs to the
    /// authoring boundary; this pass only computes reusable pixel coordinates.
    pub fn prepare(definitions: &IndexMap<FixtureDefinitionId, FixtureDefinition>) -> Self {
        Self {
            definitions: definitions
                .iter()
                .map(|(id, definition)| (*id, prepare_geometry(definition).into()))
                .collect(),
        }
    }

    pub fn pixels(&self, id: &FixtureDefinitionId) -> Option<&[PreparedPixel]> {
        self.definitions.get(id).map(AsRef::as_ref)
    }

    pub fn prepare_layout(&self, layout: &Layout) -> PreparedLayout {
        let mut prepared = PreparedLayout {
            instances: Vec::new(),
            targets: IndexMap::new(),
        };
        let mut indices = IndexMap::new();
        for fixture in layout.iter_fixtures() {
            if let LayoutFixtureKind::Fixture {
                definition,
                transform,
            } = &fixture.kind
            {
                indices.insert(fixture.id, prepared.instances.len());
                prepared.instances.push(PreparedFixtureInstance {
                    id: fixture.id,
                    pixels: self.definitions[definition].clone(),
                    transform: fixture_transform(transform),
                });
            }
        }
        for fixture in layout.iter_fixtures() {
            let mut members = Vec::new();
            for part in layout.members(fixture.id) {
                let index = indices[&part.fixture];
                if !members.contains(&index) {
                    members.push(index);
                }
            }
            prepared.targets.insert(fixture.id, members);
        }
        prepared
    }
}

/// Expand accepted fixture geometry without repeating authoring validation.
pub fn prepare_geometry(definition: &FixtureDefinition) -> Vec<PreparedPixel> {
    let mut strands = 0;
    let mut pixels = Vec::new();
    for element in &definition.elements {
        let expanded = shapes::expand_element(element);
        let first = strands;
        strands += expanded
            .iter()
            .map(|pixel| pixel.strand + 1)
            .max()
            .unwrap_or(0);
        pixels.extend(expanded.into_iter().map(|pixel| PreparedPixel {
            strand: first + pixel.strand,
            ..pixel
        }));
    }
    pixels
}

impl PreparedLayout {
    pub fn target(
        &self,
        target: FixtureId,
    ) -> Result<impl Iterator<Item = &PreparedFixtureInstance>, LayoutError> {
        let members = self
            .targets
            .get(&target)
            .ok_or(LayoutError::MissingFixture(target))?;
        Ok(members.iter().map(|&index| &self.instances[index]))
    }
}

/// The order a fixture transform's rotation turns about its axes.
const FIXTURE_EULER: EulerRot = EulerRot::XYZ;

pub fn fixture_transform(transform: &FixtureTransform) -> Affine3A {
    Affine3A::from_scale_rotation_translation(
        Vec3::new(transform.scale.x, transform.scale.y, transform.scale.z),
        Quat::from_euler(
            FIXTURE_EULER,
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

/// The fixture rotation that [`fixture_transform`] turns by `rotation`.
pub fn fixture_rotation(rotation: Quat) -> donder_language::Rotation3 {
    let (x, y, z) = rotation.to_euler(FIXTURE_EULER);
    donder_language::Rotation3 {
        x: x.to_degrees(),
        y: y.to_degrees(),
        z: z.to_degrees(),
    }
}
