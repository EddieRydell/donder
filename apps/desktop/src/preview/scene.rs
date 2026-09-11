use dawn_elaboration::fixture::PreparedFixtureDefinitions;
use dawn_language::layout::FixtureInstanceId;
use std::ops::Range;

use super::renderer::PreviewInstanceGpu;
use super::*;

#[derive(Clone, Debug)]
pub(crate) struct PreviewFixtureSpan {
    pub(crate) fixture: FixtureInstanceId,
    pub(crate) pixels: Range<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreviewScene {
    pub(crate) revision: u64,
    pub(crate) instances: Vec<PreviewInstanceGpu>,
    pub(crate) fixtures: Vec<PreviewFixtureSpan>,
    pub(crate) bounds: PreviewBounds,
}

impl PreviewScene {
    pub fn from_project(revision: u64, project: &DawnProject) -> Result<Self, String> {
        let setup = project
            .setups
            .get(&project.root.setup)
            .ok_or_else(|| "Preview setup was not found.".to_string())?;
        let layout = project
            .layouts
            .get(&setup.layout)
            .ok_or_else(|| "Preview layout was not found.".to_string())?;
        let definitions = PreparedFixtureDefinitions::prepare(&project.definitions.fixtures)
            .map_err(|error| format!("Cannot prepare preview fixtures: {error:?}"))?;
        let layout = definitions
            .prepare_layout(layout)
            .map_err(|error| format!("Cannot prepare preview layout: {error:?}"))?;
        let mut instances = Vec::new();
        let mut fixtures = Vec::new();
        for fixture in &layout.instances {
            let pixels = definitions
                .pixels(&fixture.definition)
                .ok_or_else(|| "Preview fixture definition was not prepared.".to_string())?;
            let start = instances.len();
            for pixel in pixels {
                let point = fixture.transform.transform_point3(pixel.position);
                instances.push(PreviewInstanceGpu {
                    center_radius: [point.x, point.y, pixel.diameter_meters / 2.0, 0.0],
                });
            }
            fixtures.push(PreviewFixtureSpan {
                fixture: fixture.id,
                pixels: start..instances.len(),
            });
        }
        let bounds = PreviewBounds::from_instances(&instances);
        Ok(Self {
            revision,
            instances,
            fixtures,
            bounds,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreviewBounds {
    min: Vec2,
    max: Vec2,
}

impl PreviewBounds {
    pub(crate) fn from_instances(instances: &[PreviewInstanceGpu]) -> Self {
        let Some(first) = instances.first() else {
            return Self::default();
        };
        let mut min = instance_position(first);
        let mut max = instance_position(first);
        for instance in instances.iter().skip(1) {
            let position = instance_position(instance);
            min = min.min(position);
            max = max.max(position);
        }
        Self { min, max }
    }
}

pub(crate) fn instance_position(instance: &PreviewInstanceGpu) -> Vec2 {
    Vec2::new(instance.center_radius[0], instance.center_radius[1])
}

impl Default for PreviewBounds {
    fn default() -> Self {
        Self {
            min: Vec2::ZERO,
            max: Vec2::new(1.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreviewCamera {
    pub(crate) pan: Vec2,
    pub(crate) zoom: f32,
}

impl PreviewCamera {
    pub(crate) fn fit(bounds: PreviewBounds, size: PreviewSize) -> Self {
        let span = (bounds.max - bounds.min).max(Vec2::splat(1.0));
        let available = Vec2::new(size.width as f32, size.height as f32) * 0.82;
        let zoom = (available.x / span.x).min(available.y / span.y).max(1.0);
        Self {
            pan: (bounds.min + bounds.max) * 0.5,
            zoom,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PreviewSize {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl PreviewSize {
    pub(crate) fn clamp_to_max_dimension(self, max_dimension: u32) -> Self {
        Self {
            width: self.width.min(max_dimension),
            height: self.height.min(max_dimension),
        }
    }
}
