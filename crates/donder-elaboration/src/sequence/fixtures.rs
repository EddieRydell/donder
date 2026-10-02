use donder_language::layout::{FixtureInstanceId, Layout, LayoutFixture, LayoutFixtureKind};
use donder_language::model::DonderProject;
use indexmap::IndexMap;

#[derive(Clone, Debug)]
pub(crate) struct PreparedFixture {
    pub(crate) id: FixtureInstanceId,
    pub(crate) pixel_count: usize,
    pub(crate) positions: Vec<[f32; 2]>,
}

pub(crate) type PreparedFixtures = (
    Vec<PreparedFixture>,
    IndexMap<FixtureInstanceId, Vec<FixtureInstanceId>>,
);

pub(crate) fn prepare_fixtures(project: &DonderProject, layout: &Layout) -> PreparedFixtures {
    let geometry = donder_language::geometry::PreparedFixtureDefinitions::prepare(
        &project.definitions.fixtures,
    )
    .prepare_layout(layout);
    fn visit(
        nodes: &[LayoutFixture],
        geometry: &donder_language::geometry::PreparedLayout,
        fixtures: &mut Vec<PreparedFixture>,
        groups: &mut IndexMap<FixtureInstanceId, Vec<FixtureInstanceId>>,
    ) {
        for fixture in nodes {
            match &fixture.kind {
                LayoutFixtureKind::Fixture { .. } => {
                    // Geometry and playback use the same layout traversal. Take
                    // both counts and positions from that one expansion.
                    let instance = &geometry.instances[fixtures.len()];
                    fixtures.push(PreparedFixture {
                        id: fixture.id,
                        pixel_count: instance.pixels.len(),
                        positions: instance
                            .pixels
                            .iter()
                            .map(|pixel| {
                                let point = instance.transform.transform_point3(pixel.position);
                                [point.x, point.y]
                            })
                            .collect(),
                    });
                }
                LayoutFixtureKind::Group { children } => {
                    let start = fixtures.len();
                    visit(children, geometry, fixtures, groups);
                    groups.insert(
                        fixture.id,
                        fixtures[start..].iter().map(|fixture| fixture.id).collect(),
                    );
                }
            }
        }
    }
    let mut fixtures = Vec::new();
    let mut groups = IndexMap::new();
    visit(&layout.fixtures, &geometry, &mut fixtures, &mut groups);
    (fixtures, groups)
}
