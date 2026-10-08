use super::*;

/// Duplicate a placement or group with independently owned geometry. A group
/// copies its members once each, so members shared inside it stay shared. The
/// copy is listed after the original in every group listing the original.
/// Existing routes and effects keep targeting the originals.
pub fn duplicate_layout_fixture(
    project: &mut DonderProject,
    layout_id: &LayoutId,
    fixture_id: FixtureInstanceId,
) -> Result<FixtureInstanceId, String> {
    project.checked_edit(|project| duplicate_candidate(project, layout_id, fixture_id))
}

struct Copier<'a> {
    project: &'a DonderProject,
    layout: &'a Layout,
    next: u32,
    taken: std::collections::HashSet<donder_runtime_types::Identifier>,
    copies: indexmap::IndexMap<FixtureInstanceId, LayoutFixture>,
}

impl Copier<'_> {
    // Every copied placement and group gets its own unique name.
    fn own(&mut self, id: FixtureInstanceId) -> Result<FixtureInstanceId, String> {
        if let Some(copy) = self.copies.get(&id) {
            return Ok(copy.id);
        }
        let mut copy = self
            .layout
            .fixture(id)
            .ok_or("Fixture was not found.")?
            .clone();
        self.next = self
            .next
            .checked_add(1)
            .ok_or("No fixture instance IDs remain.")?;
        copy.id = FixtureInstanceId(self.next);
        copy.name = donder_language::unique_name(&format!("{}_copy", copy.name.as_str()), |name| {
            self.taken.iter().any(|taken| taken.as_str() == name)
        });
        self.taken.insert(copy.name.clone());
        match &mut copy.kind {
            LayoutFixtureKind::Fixture { definition, .. } => {
                if let FixtureSource::Reference(id) = definition {
                    *definition = FixtureSource::Inline(
                        self.project
                            .definitions
                            .fixtures
                            .definitions
                            .get(id)
                            .ok_or("Fixture source was not found.")?
                            .clone(),
                    );
                }
            }
            LayoutFixtureKind::Group { members } => {
                *members = members
                    .iter()
                    .map(|&member| self.own(member))
                    .collect::<Result<_, _>>()?;
            }
        }
        let copied = copy.id;
        self.copies.insert(id, copy);
        Ok(copied)
    }
}

fn duplicate_candidate(
    project: &mut DonderProject,
    layout_id: &LayoutId,
    fixture_id: FixtureInstanceId,
) -> Result<FixtureInstanceId, String> {
    let layout = project.layout(layout_id).ok_or("Layout was not found.")?;
    let mut copier = Copier {
        project,
        layout,
        next: layout
            .iter_fixtures()
            .map(|fixture| fixture.id.0)
            .max()
            .unwrap_or(0),
        taken: layout
            .iter_fixtures()
            .map(|fixture| fixture.name.clone())
            .collect(),
        copies: indexmap::IndexMap::new(),
    };
    let id = copier.own(fixture_id)?;
    let copies = copier.copies;
    let layout = project
        .layout_mut(layout_id)
        .ok_or("Layout was not found.")?;
    for (original, copy) in copies {
        let index = layout
            .fixtures
            .iter()
            .position(|fixture| fixture.id == original)
            .ok_or("Fixture was not found.")?;
        layout.fixtures.insert(index + 1, copy);
    }
    layout.add_beside(id, fixture_id);
    Ok(id)
}
