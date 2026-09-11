use crate::RenderError;
use dawn_language::layout::{FixtureInstanceId, Layout, LayoutFixture, LayoutFixtureKind};
use dawn_language::model::DawnProject;
use indexmap::IndexMap;

#[derive(Clone, Debug)]
pub(crate) struct PreparedFixture {
    pub(crate) id: FixtureInstanceId,
    pub(crate) pixel_count: usize,
}

pub(crate) type PreparedFixtures = (
    Vec<PreparedFixture>,
    IndexMap<FixtureInstanceId, Vec<FixtureInstanceId>>,
);

pub(crate) fn prepare_fixtures(
    project: &DawnProject,
    layout: &Layout,
) -> Result<PreparedFixtures, RenderError> {
    let counts = project
        .definitions
        .fixtures
        .pixel_counts()
        .map_err(|error| RenderError::BadGraph {
            message: format!("Invalid fixture composition: {error:?}"),
        })?;
    layout
        .validate(&counts)
        .map_err(|error| RenderError::BadGraph {
            message: format!("Invalid layout: {error:?}"),
        })?;
    fn visit(
        nodes: &[LayoutFixture],
        counts: &IndexMap<dawn_language::fixture::FixtureDefinitionId, u32>,
        fixtures: &mut Vec<PreparedFixture>,
        groups: &mut IndexMap<FixtureInstanceId, Vec<FixtureInstanceId>>,
    ) {
        for fixture in nodes {
            match &fixture.kind {
                LayoutFixtureKind::Fixture { definition, .. } => fixtures.push(PreparedFixture {
                    id: fixture.id,
                    pixel_count: counts[definition] as usize,
                }),
                LayoutFixtureKind::Group { children } => {
                    let start = fixtures.len();
                    visit(children, counts, fixtures, groups);
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
    visit(&layout.fixtures, &counts, &mut fixtures, &mut groups);
    Ok((fixtures, groups))
}
