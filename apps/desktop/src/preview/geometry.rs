use crate::dto::Point3Meters;
use donder_elaboration::fixture::PreparedFixtureDefinitions;
use donder_language::model::DonderProject;
use donder_language::values::Point3;

#[derive(Clone, Debug)]
pub(crate) struct PreviewGeometry {
    pub(crate) instances: Vec<[f32; 4]>,
    pub(crate) fixtures: Vec<(u32, usize)>,
}

impl PreviewGeometry {
    pub(crate) fn from_project(project: &DonderProject) -> Result<Self, String> {
        let setup = project
            .setup(project.root.setup.id())
            .ok_or_else(|| "Preview setup was not found.".to_string())?;
        let layout = project
            .layout(setup.layout.id())
            .ok_or_else(|| "Preview layout was not found.".to_string())?;
        let definitions = PreparedFixtureDefinitions::prepare(&project.definitions.fixtures)
            .map_err(|error| format!("Cannot prepare preview fixtures: {error:?}"))?;
        let layout = definitions
            .prepare_layout(layout)
            .map_err(|error| format!("Cannot prepare preview layout: {error:?}"))?;
        let mut instances = Vec::new();
        let mut fixtures = Vec::new();
        for fixture in &layout.instances {
            let pixels = fixture.pixels.as_ref();
            for pixel in pixels {
                let point = fixture.transform.transform_point3(pixel.position);
                instances.push([point.x, point.y, pixel.diameter_meters / 2.0, 0.0]);
            }
            fixtures.push((fixture.id.0, pixels.len()));
        }
        Ok(Self {
            instances,
            fixtures,
        })
    }
}

pub(crate) fn point3_meters(point: Point3) -> Point3Meters {
    Point3Meters {
        x_meters: point.x.as_meters_f32(),
        y_meters: point.y.as_meters_f32(),
        z_meters: point.z.as_meters_f32(),
    }
}
