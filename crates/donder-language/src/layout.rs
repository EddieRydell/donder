//! Layout fixtures are effect targets. Parts inside a definition are not layout targets.

mod editing;

use std::collections::HashSet;

use crate::fixture::{FixtureDefinitionId, FixtureGeometryError, FixtureSource, FixtureTransform};
use crate::identity::ObjectIdentity;
use indexmap::IndexMap;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LayoutId(pub ObjectIdentity);

/// Stable across renames, reordering and group moves; unique in a layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FixtureInstanceId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub id: LayoutId,
    pub description: Option<String>,
    pub fixtures: Vec<LayoutFixture>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutFixture {
    pub id: FixtureInstanceId,
    /// Unique within the layout, at any depth; references use it.
    pub name: crate::dsl::Identifier,
    pub description: Option<String>,
    pub kind: LayoutFixtureKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LayoutFixtureKind {
    Fixture {
        definition: FixtureSource,
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
    InvalidName(FixtureInstanceId),
    DuplicateName(crate::dsl::Identifier),
    MissingDefinition(FixtureDefinitionId),
    InvalidFixture {
        fixture: FixtureInstanceId,
        error: FixtureGeometryError,
    },
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
                LayoutFixtureKind::Fixture { definition, .. } => match definition {
                    FixtureSource::Inline(value) => {
                        value
                            .validate_geometry()
                            .map_err(|error| LayoutError::InvalidFixture {
                                fixture: fixture.id,
                                error,
                            })
                    }
                    FixtureSource::Reference(id) => counts
                        .get(id)
                        .copied()
                        .ok_or_else(|| LayoutError::MissingDefinition(id.clone())),
                },
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
            names: &mut HashSet<crate::dsl::Identifier>,
        ) -> Result<(), LayoutError> {
            for fixture in fixtures {
                if !seen.insert(fixture.id) {
                    return Err(LayoutError::DuplicateId(fixture.id));
                }
                if !crate::names::is_object_name(fixture.name.as_str()) {
                    return Err(LayoutError::InvalidName(fixture.id));
                }
                if !names.insert(fixture.name.clone()) {
                    return Err(LayoutError::DuplicateName(fixture.name.clone()));
                }
                match &fixture.kind {
                    LayoutFixtureKind::Fixture {
                        definition,
                        transform,
                    } => {
                        match definition {
                            FixtureSource::Inline(value) => {
                                value.validate_geometry().map_err(|error| {
                                    LayoutError::InvalidFixture {
                                        fixture: fixture.id,
                                        error,
                                    }
                                })?;
                            }
                            FixtureSource::Reference(id) if !definitions.contains_key(id) => {
                                return Err(LayoutError::MissingDefinition(id.clone()));
                            }
                            FixtureSource::Reference(_) => {}
                        }
                        if !transform.is_valid() {
                            return Err(LayoutError::InvalidTransform(fixture.id));
                        }
                    }
                    LayoutFixtureKind::Group { children } => {
                        visit(children, definitions, seen, names)?
                    }
                }
            }
            Ok(())
        }
        visit(
            &self.fixtures,
            definitions,
            &mut HashSet::new(),
            &mut HashSet::new(),
        )
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

impl crate::ownership::Identified for Layout {
    type Id = LayoutId;
    fn id(&self) -> &Self::Id {
        &self.id
    }
}

pub type LayoutSource = crate::ownership::ValueSource<Box<Layout>, LayoutId>;

impl AsRef<ObjectIdentity> for LayoutId {
    fn as_ref(&self) -> &ObjectIdentity {
        &self.0
    }
}
