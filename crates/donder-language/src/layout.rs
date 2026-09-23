//! Layout fixtures are effect targets. Parts inside a definition are not layout targets.

use std::collections::HashSet;

use crate::fixture::{FixtureDefinitionId, FixtureTransform};
use crate::identity::SourceIdentity;
use indexmap::IndexMap;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LayoutId(pub SourceIdentity);

/// Stable across renames, reordering and group moves; unique in a layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FixtureInstanceId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub id: LayoutId,
    pub fixtures: Vec<LayoutFixture>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutFixture {
    pub id: FixtureInstanceId,
    pub name: String,
    pub kind: LayoutFixtureKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LayoutFixtureKind {
    Fixture {
        definition: FixtureDefinitionId,
        transform: FixtureTransform,
    },
    Group {
        children: Vec<LayoutFixture>,
    },
}

/// Select an entire fixture or group. There is no effect pixel-range selector.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FixtureTarget {
    pub layout: LayoutId,
    pub fixture: FixtureInstanceId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutError {
    TooManyPixels(FixtureInstanceId),
    DuplicateId(FixtureInstanceId),
    EmptyName(FixtureInstanceId),
    MissingDefinition(FixtureDefinitionId),
    InvalidTransform(FixtureInstanceId),
    MissingFixture(FixtureInstanceId),
    WrongLayout(LayoutId),
}

impl Layout {
    pub fn iter_fixtures(&self) -> impl Iterator<Item = &LayoutFixture> {
        let mut stack = vec![self.fixtures.iter()];
        std::iter::from_fn(move || {
            loop {
                let next = stack.last_mut()?.next();
                if let Some(fixture) = next {
                    if let LayoutFixtureKind::Group { children } = &fixture.kind {
                        stack.push(children.iter());
                    }
                    return Some(fixture);
                }
                stack.pop();
            }
        })
    }

    pub fn target_pixel_count(
        &self,
        target: &FixtureTarget,
        counts: &IndexMap<FixtureDefinitionId, u32>,
    ) -> Result<u32, LayoutError> {
        fn count(
            fixture: &LayoutFixture,
            counts: &IndexMap<FixtureDefinitionId, u32>,
        ) -> Result<u32, LayoutError> {
            match &fixture.kind {
                LayoutFixtureKind::Fixture { definition, .. } => counts
                    .get(definition)
                    .copied()
                    .ok_or_else(|| LayoutError::MissingDefinition(definition.clone())),
                LayoutFixtureKind::Group { children } => {
                    children.iter().try_fold(0u32, |total, child| {
                        total
                            .checked_add(count(child, counts)?)
                            .ok_or(LayoutError::TooManyPixels(fixture.id))
                    })
                }
            }
        }
        if target.layout != self.id {
            return Err(LayoutError::WrongLayout(target.layout.clone()));
        }
        count(
            self.fixture(target.fixture)
                .ok_or(LayoutError::MissingFixture(target.fixture))?,
            counts,
        )
    }

    pub fn validate<T>(
        &self,
        definitions: &IndexMap<FixtureDefinitionId, T>,
    ) -> Result<(), LayoutError> {
        fn visit<T>(
            fixtures: &[LayoutFixture],
            definitions: &IndexMap<FixtureDefinitionId, T>,
            seen: &mut HashSet<FixtureInstanceId>,
        ) -> Result<(), LayoutError> {
            for fixture in fixtures {
                if !seen.insert(fixture.id) {
                    return Err(LayoutError::DuplicateId(fixture.id));
                }
                if fixture.name.trim().is_empty() {
                    return Err(LayoutError::EmptyName(fixture.id));
                }
                match &fixture.kind {
                    LayoutFixtureKind::Fixture {
                        definition,
                        transform,
                    } => {
                        if !definitions.contains_key(definition) {
                            return Err(LayoutError::MissingDefinition(definition.clone()));
                        }
                        if !transform.is_valid() {
                            return Err(LayoutError::InvalidTransform(fixture.id));
                        }
                    }
                    LayoutFixtureKind::Group { children } => visit(children, definitions, seen)?,
                }
            }
            Ok(())
        }
        visit(&self.fixtures, definitions, &mut HashSet::new())
    }

    pub fn fixture(&self, id: FixtureInstanceId) -> Option<&LayoutFixture> {
        fn find(fixtures: &[LayoutFixture], id: FixtureInstanceId) -> Option<&LayoutFixture> {
            fixtures.iter().find_map(|fixture| {
                if fixture.id == id {
                    return Some(fixture);
                }
                match &fixture.kind {
                    LayoutFixtureKind::Group { children } => find(children, id),
                    LayoutFixtureKind::Fixture { .. } => None,
                }
            })
        }
        find(&self.fixtures, id)
    }
}
