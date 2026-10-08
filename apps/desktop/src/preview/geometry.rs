use donder_model::DonderProject;
use donder_model::PreparedFixtureDefinitions;

#[derive(Clone, Debug)]
pub(crate) struct PreviewGeometry {
    pub(crate) instances: Vec<[f32; 4]>,
    pub(crate) fixtures: Vec<(u32, usize)>,
}

impl PreviewGeometry {
    pub(crate) fn from_project(project: &DonderProject) -> Result<Self, String> {
        let setup = project
            .setup(project.root().setup.id())
            .ok_or_else(|| "Preview setup was not found.".to_string())?;
        let layout = project
            .layout(setup.layout.id())
            .ok_or_else(|| "Preview layout was not found.".to_string())?;
        let definitions = PreparedFixtureDefinitions::prepare(&project.definitions().fixtures);
        let layout = definitions.prepare_layout(layout);
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
