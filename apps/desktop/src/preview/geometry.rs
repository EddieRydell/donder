use donder_model::DonderProject;
use donder_preview::FrontView;

#[derive(Clone, Debug)]
pub(crate) struct PreviewGeometry {
    pub(crate) instances: Vec<[f32; 4]>,
    pub(crate) fixtures: Vec<(u32, usize)>,
}

impl PreviewGeometry {
    pub(crate) fn from_project(project: &DonderProject) -> Result<Self, String> {
        let view = FrontView::from_project(project).map_err(|error| error.to_string())?;
        let instances = view
            .pixels()
            .map(|pixel| [pixel.position.x, pixel.position.y, pixel.radius, 0.0])
            .collect();
        let fixtures = view
            .fixtures
            .iter()
            .map(|fixture| (fixture.id, fixture.pixels.len()))
            .collect();
        Ok(Self {
            instances,
            fixtures,
        })
    }
}
