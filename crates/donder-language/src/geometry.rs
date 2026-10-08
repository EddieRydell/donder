//! Shared fixture geometry for authoring previews and sequence preparation.
//!
//! Expands shapes and layout transforms without compiling effects or creating
//! playback state. Both the editor and elaboration use this same geometry.

use std::sync::Arc;

use crate::fixture::{
    FixtureDefinition, FixtureDefinitionId, FixtureDefinitions, FixtureElementId, FixtureSource,
    FixtureTransform,
};
use crate::layout::{
    FixtureInstanceId, FixtureTarget, Layout, LayoutError, LayoutFixtureKind, LayoutId,
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
    /// Each target selects its member instances in target order.
    targets: IndexMap<FixtureInstanceId, Vec<usize>>,
}

impl PreparedFixtureDefinitions {
    /// Expand definitions from an accepted project. Validation belongs to the
    /// authoring boundary; this pass only computes reusable pixel coordinates.
    pub fn prepare(definitions: &FixtureDefinitions) -> Self {
        Self {
            definitions: definitions
                .definitions
                .iter()
                .map(|(id, definition)| (id.clone(), prepare_geometry(definition).into()))
                .collect(),
        }
    }

    pub fn pixels(&self, id: &FixtureDefinitionId) -> Option<&[PreparedPixel]> {
        self.definitions.get(id).map(AsRef::as_ref)
    }

    pub fn prepare_layout(&self, layout: &Layout) -> PreparedLayout {
        let mut prepared = PreparedLayout {
            id: layout.id.clone(),
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
                    pixels: match definition {
                        FixtureSource::Inline(value) => prepare_geometry(value).into(),
                        FixtureSource::Reference(id) => self.definitions[id].clone(),
                    },
                    transform: fixture_transform(transform),
                });
            }
        }
        for fixture in layout.iter_fixtures() {
            let members = layout
                .members(fixture.id)
                .iter()
                .map(|id| indices[id])
                .collect();
            prepared.targets.insert(fixture.id, members);
        }
        prepared
    }
}

/// Expand accepted fixture geometry without repeating authoring validation.
pub fn prepare_geometry(definition: &FixtureDefinition) -> Vec<PreparedPixel> {
    definition
        .elements
        .iter()
        .flat_map(shapes::expand_element)
        .collect()
}

impl PreparedLayout {
    pub fn target(
        &self,
        target: &FixtureTarget,
    ) -> Result<impl Iterator<Item = &PreparedFixtureInstance>, LayoutError> {
        if target.layout != self.id {
            return Err(LayoutError::WrongLayout(target.layout.clone()));
        }
        let members = self
            .targets
            .get(&target.fixture)
            .ok_or(LayoutError::MissingFixture(target.fixture))?;
        Ok(members.iter().map(|&index| &self.instances[index]))
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
