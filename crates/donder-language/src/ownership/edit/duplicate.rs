use super::*;

/// Duplicate a placement or group with independently owned geometry.
/// Existing routes and effects keep targeting the originals.
pub fn duplicate_layout_fixture(
    project: &mut DonderProject,
    layout_id: &LayoutId,
    fixture_id: FixtureInstanceId,
) -> Result<FixtureInstanceId, String> {
    let layout = project.layout(layout_id).ok_or("Layout was not found.")?;
    let mut copy = layout
        .fixture(fixture_id)
        .ok_or("Fixture was not found.")?
        .clone();
    let mut next = layout
        .iter_fixtures()
        .map(|fixture| fixture.id.0)
        .max()
        .unwrap_or(0);
    fn own(
        fixture: &mut LayoutFixture,
        next: &mut u32,
        project: &DonderProject,
    ) -> Result<(), String> {
        *next = next
            .checked_add(1)
            .ok_or("No fixture instance IDs remain.")?;
        fixture.id = FixtureInstanceId(*next);
        match &mut fixture.kind {
            LayoutFixtureKind::Fixture { definition, .. } => {
                if let FixtureSource::Reference(id) = definition {
                    *definition = FixtureSource::Inline(
                        project
                            .definitions
                            .fixtures
                            .definitions
                            .get(id)
                            .ok_or("Fixture source was not found.")?
                            .clone(),
                    );
                }
            }
            LayoutFixtureKind::Group { children } => {
                for child in children {
                    own(child, next, project)?;
                }
            }
        }
        Ok(())
    }
    own(&mut copy, &mut next, project)?;
    copy.name = format!("{} copy", copy.name);
    let id = copy.id;
    fn siblings(
        fixtures: &mut Vec<LayoutFixture>,
        id: FixtureInstanceId,
    ) -> Option<(&mut Vec<LayoutFixture>, usize)> {
        if let Some(index) = fixtures.iter().position(|fixture| fixture.id == id) {
            return Some((fixtures, index));
        }
        for fixture in fixtures {
            if let LayoutFixtureKind::Group { children } = &mut fixture.kind
                && let Some(found) = siblings(children, id)
            {
                return Some(found);
            }
        }
        None
    }
    let layout = project
        .layout_mut(layout_id)
        .ok_or("Layout was not found.")?;
    let (siblings, index) =
        siblings(&mut layout.fixtures, fixture_id).ok_or("Fixture was not found.")?;
    siblings.insert(index + 1, copy);
    Ok(id)
}
