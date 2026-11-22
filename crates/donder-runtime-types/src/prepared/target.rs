//! Sampling domains preserve their original coordinates when storage is selected.
//! These descriptions contain geometry only, never executable graph references.
use super::{FixtureGeometry, SpatialContext, TargetScope};
use alloc::{boxed::Box, collections::BTreeSet, vec::Vec};
use core::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetPixel {
    fixture: usize,
    cell: u32,
    source_cell: u32,
    index: usize,
    count: usize,
    spatial: SpatialContext,
}

impl TargetPixel {
    pub fn fixture(&self) -> usize {
        self.fixture
    }
    pub fn cell(&self) -> u32 {
        self.cell
    }
    pub fn source_cell(&self) -> u32 {
        self.source_cell
    }
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn spatial(&self) -> SpatialContext {
        self.spatial
    }
}

/// Original target population and its selected physical storage addresses.
/// Fixture indices refer to the supplied geometry inventory, not runtime storage.
#[derive(Clone, PartialEq)]
pub struct TargetGeometry {
    pixels: Box<[TargetPixel]>,
    selection: Box<[(usize, u32)]>,
    scope: TargetScope,
}

impl TargetGeometry {
    /// Whole fixtures; the first occurrence of each fixture determines
    /// whole-target ordering.
    pub fn new<'a>(
        fixtures: impl IntoIterator<Item = (usize, &'a FixtureGeometry)>,
        scope: TargetScope,
    ) -> Self {
        Self::parts(
            fixtures.into_iter().map(|(fixture, geometry)| {
                (fixture, geometry, 0..geometry.positions().len() as u32)
            }),
            scope,
        )
    }

    /// Runs of fixtures' cells in order. A cell reached more than once keeps
    /// its first position; per fixture, each part counts as its own fixture.
    pub fn parts<'a>(
        parts: impl IntoIterator<Item = (usize, &'a FixtureGeometry, Range<u32>)>,
        scope: TargetScope,
    ) -> Self {
        let mut seen = BTreeSet::new();
        let parts: Vec<(usize, &FixtureGeometry, Vec<u32>)> = parts
            .into_iter()
            .map(|(fixture, geometry, cells)| {
                let end = cells.end.min(geometry.positions().len() as u32);
                let cells = (cells.start..end)
                    .filter(|&cell| seen.insert((fixture, cell)))
                    .collect();
                (fixture, geometry, cells)
            })
            .filter(|(_, _, cells): &(usize, &FixtureGeometry, Vec<u32>)| !cells.is_empty())
            .collect();
        let total = parts.iter().map(|(_, _, cells)| cells.len()).sum();
        let position = |geometry: &FixtureGeometry, cell: u32| geometry.positions()[cell as usize];
        let whole =
            bounds(parts.iter().flat_map(|(_, geometry, cells)| {
                cells.iter().map(|&cell| position(geometry, cell))
            }));
        let mut pixels = Vec::with_capacity(total);
        let mut selection = Vec::with_capacity(total);
        let mut logical_offset = 0;
        for (fixture, geometry, cells) in parts {
            let extent = match scope {
                TargetScope::PerFixture => {
                    bounds(cells.iter().map(|&cell| position(geometry, cell)))
                }
                TargetScope::WholeTarget => whole,
            };
            if let Some((min, max)) = extent {
                for (offset, &cell) in cells.iter().enumerate() {
                    selection.push((fixture, cell));
                    let Ok(storage_cell) = geometry.cells().binary_search(&cell) else {
                        continue;
                    };
                    let (index, count) = match scope {
                        TargetScope::PerFixture => (offset, cells.len()),
                        TargetScope::WholeTarget => (logical_offset + offset, total),
                    };
                    pixels.push(TargetPixel {
                        fixture,
                        cell: storage_cell as u32,
                        source_cell: cell,
                        index,
                        count,
                        spatial: SpatialContext {
                            position: position(geometry, cell),
                            min,
                            max,
                        },
                    });
                }
            }
            logical_offset += cells.len();
        }
        pixels.sort_by_key(|pixel| (pixel.fixture, pixel.cell));
        Self {
            pixels: pixels.into(),
            selection: selection.into(),
            scope,
        }
    }

    pub fn pixels(&self) -> &[TargetPixel] {
        &self.pixels
    }
    pub fn selection(&self) -> &[(usize, u32)] {
        &self.selection
    }
    pub fn scope(&self) -> TargetScope {
        self.scope
    }

    /// Intersect a physical target range before storage selection. This changes
    /// the target population, while retaining its original sampling coordinates.
    pub fn slice(&self, range: core::ops::Range<usize>) -> Self {
        let pixels: Box<[_]> = self
            .pixels
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
            .map(|(_, pixel)| *pixel)
            .collect();
        let selected: BTreeSet<_> = pixels
            .iter()
            .map(|pixel| (pixel.fixture, pixel.source_cell))
            .collect();
        Self {
            pixels,
            selection: self
                .selection
                .iter()
                .filter(|address| selected.contains(address))
                .copied()
                .collect(),
            scope: self.scope,
        }
    }
}

fn bounds(points: impl IntoIterator<Item = [f32; 2]>) -> Option<([f32; 2], [f32; 2])> {
    let mut points = points.into_iter();
    let first = points.next()?;
    Some(points.fold((first, first), |(mut min, mut max), point| {
        for axis in 0..2 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
        (min, max)
    }))
}
