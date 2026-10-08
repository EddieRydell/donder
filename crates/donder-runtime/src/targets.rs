//! Compact physical selections with their original logical sampling coordinates.
//! These structures are both the archive and the executable representation.
use alloc::{boxed::Box, vec::Vec};
use donder_runtime_types::Shared;

use crate::dsl::SpatialContext;
use crate::signal::PreparedPixel;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedTarget {
    pub pixels: TargetPixels,
    pub spatial: Box<[SpatialRun]>,
    pub sections: crate::sections::PreparedSections,
    pub sample_count: usize,
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum TargetPixels {
    Indexed(Shared<[PreparedPixel]>),
    Runs(Shared<[PixelRun]>),
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PixelRun {
    end: usize,
    fixture: usize,
    first_cell: u32,
    first_index: usize,
    count: usize,
    // Exact compiler-produced fractions, shared between identical runs. This
    // avoids both runtime division and changed rounding from a reciprocal.
    fractions: Shared<[f32]>,
}

impl PixelRun {
    #[inline]
    fn pixel(&self, offset: usize) -> PreparedPixel {
        PreparedPixel {
            fixture_index: self.fixture,
            fixture_pixel_index: self.first_cell + offset as u32,
            pixel_index: self.first_index + offset,
            pixel_count: self.count,
            pixel_fraction: self.fractions[offset],
        }
    }
}

impl TargetPixels {
    #[inline]
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Indexed(pixels) => pixels.len(),
            Self::Runs(runs) => runs.last().map_or(0, |run| run.end),
        }
    }

    #[inline]
    pub(crate) fn pixel(&self, index: usize) -> PreparedPixel {
        match self {
            Self::Indexed(pixels) => pixels[index],
            Self::Runs(runs) => {
                if let [run] = runs.as_ref() {
                    return run.pixel(index);
                }
                let run = runs.partition_point(|run| run.end <= index);
                let start = if run == 0 { 0 } else { runs[run - 1].end };
                runs[run].pixel(index - start)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn last(&self) -> Option<PreparedPixel> {
        self.len().checked_sub(1).map(|index| self.pixel(index))
    }

    /// Physical cells are sorted even when logical target order differs.
    pub(crate) fn find(&self, fixture: usize, cell: u32) -> Option<usize> {
        match self {
            Self::Indexed(pixels) => pixels
                .binary_search_by_key(&(fixture, cell), |p| {
                    (p.fixture_index, p.fixture_pixel_index)
                })
                .ok(),
            Self::Runs(runs) => {
                let run = runs
                    .partition_point(|r| (r.fixture, r.first_cell) <= (fixture, cell))
                    .checked_sub(1)?;
                let start = if run == 0 { 0 } else { runs[run - 1].end };
                let r = &runs[run];
                let offset = cell.checked_sub(r.first_cell)? as usize;
                (r.fixture == fixture && offset < r.end - start).then_some(start + offset)
            }
        }
    }

    /// Consecutive target pixels that share a fixture, count and contiguous
    /// physical cells. Indexed targets yield one pixel per segment.
    pub(crate) fn segments(&self) -> impl Iterator<Item = Segment<'_>> {
        let (indexed, runs): (&[PreparedPixel], &[PixelRun]) = match self {
            Self::Indexed(pixels) => (pixels, &[]),
            Self::Runs(runs) => (&[], runs),
        };
        let indexed = indexed.iter().enumerate().map(|(start, pixel)| Segment {
            start,
            fixture: pixel.fixture_index,
            first_cell: pixel.fixture_pixel_index,
            first_index: pixel.pixel_index,
            count: pixel.pixel_count,
            fractions: core::slice::from_ref(&pixel.pixel_fraction),
        });
        let runs = runs.iter().scan(0, |start, run| {
            let segment = Segment {
                start: *start,
                fixture: run.fixture,
                first_cell: run.first_cell,
                first_index: run.first_index,
                count: run.count,
                fractions: &run.fractions[..run.end - *start],
            };
            *start = run.end;
            Some(segment)
        });
        indexed.chain(runs)
    }

    pub(crate) fn iter(&self) -> TargetIter<'_> {
        TargetIter {
            pixels: self,
            index: 0,
            run: 0,
            start: 0,
        }
    }
}

pub(crate) struct Segment<'a> {
    /// Target index of the first pixel.
    pub start: usize,
    pub fixture: usize,
    pub first_cell: u32,
    pub first_index: usize,
    pub count: usize,
    pub fractions: &'a [f32],
}

impl Segment<'_> {
    pub(crate) fn pixel(&self, offset: usize) -> PreparedPixel {
        PreparedPixel {
            fixture_index: self.fixture,
            fixture_pixel_index: self.first_cell + offset as u32,
            pixel_index: self.first_index + offset,
            pixel_count: self.count,
            pixel_fraction: self.fractions[offset],
        }
    }
}

pub(crate) struct TargetIter<'a> {
    pixels: &'a TargetPixels,
    index: usize,
    run: usize,
    start: usize,
}

impl Iterator for TargetIter<'_> {
    type Item = PreparedPixel;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let pixel = match self.pixels {
            TargetPixels::Indexed(pixels) => *pixels.get(self.index)?,
            TargetPixels::Runs(runs) => {
                if runs.get(self.run).is_some_and(|run| run.end == self.index) {
                    self.start = self.index;
                    self.run += 1;
                }
                runs.get(self.run)?.pixel(self.index - self.start)
            }
        };
        self.index += 1;
        Some(pixel)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.pixels.len() - self.index;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for TargetIter<'_> {}

impl<'a> IntoIterator for &'a TargetPixels {
    type Item = PreparedPixel;
    type IntoIter = TargetIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Consecutive pixels with the same original scope bounds share one record.
#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct SpatialRun {
    end: usize,
    min: [f32; 2],
    max: [f32; 2],
}

impl PreparedTarget {
    pub(crate) fn prepare_spatial(contexts: &[SpatialContext]) -> Box<[SpatialRun]> {
        let mut runs = Vec::<SpatialRun>::new();
        for (index, context) in contexts.iter().enumerate() {
            if let Some(run) = runs.last_mut()
                && run.min.map(f32::to_bits) == context.min.map(f32::to_bits)
                && run.max.map(f32::to_bits) == context.max.map(f32::to_bits)
            {
                run.end = index + 1;
            } else {
                runs.push(SpatialRun {
                    end: index + 1,
                    min: context.min,
                    max: context.max,
                });
            }
        }
        runs.into()
    }

    pub(crate) fn spatial_context(
        &self,
        index: usize,
        pixel: &PreparedPixel,
        positions: &[Box<[[f32; 2]]>],
    ) -> SpatialContext {
        let run = &self.spatial[self.spatial.partition_point(|run| run.end <= index)];
        SpatialContext {
            position: positions[pixel.fixture_index][pixel.fixture_pixel_index as usize],
            min: run.min,
            max: run.max,
        }
    }
}

/// Preparation-only sharing. Playback owns only the referenced immutable tables.
#[derive(Default)]
pub(crate) struct TargetInterner {
    pixels: Vec<Shared<[PreparedPixel]>>,
    fractions: Vec<Shared<[f32]>>,
    runs: Vec<Shared<[PixelRun]>>,
}

impl TargetInterner {
    pub(crate) fn prepare(&mut self, pixels: Vec<PreparedPixel>) -> TargetPixels {
        let mut spans = Vec::new();
        let mut start = 0;
        for index in 1..=pixels.len() {
            if index < pixels.len() {
                let before = &pixels[index - 1];
                let next = &pixels[index];
                if before.fixture_index == next.fixture_index
                    && before.fixture_pixel_index.checked_add(1) == Some(next.fixture_pixel_index)
                    && before.pixel_index.checked_add(1) == Some(next.pixel_index)
                    && before.pixel_count == next.pixel_count
                {
                    continue;
                }
            }
            spans.push(start..index);
            start = index;
        }
        // Compare the actual archive representations, independently of host
        // pointer width. Irregular mappings use the smaller explicit form.
        let run_bytes = spans.len() * core::mem::size_of::<rkyv::Archived<PixelRun>>()
            + pixels.len() * core::mem::size_of::<f32>();
        let indexed_bytes = pixels.len() * core::mem::size_of::<rkyv::Archived<PreparedPixel>>();
        if run_bytes >= indexed_bytes {
            if let Some(existing) = self
                .pixels
                .iter()
                .find(|existing| existing.as_ref() == pixels)
            {
                return TargetPixels::Indexed(Shared::clone(existing));
            }
            let pixels: Shared<[PreparedPixel]> = pixels.into();
            self.pixels.push(Shared::clone(&pixels));
            return TargetPixels::Indexed(pixels);
        }
        let runs: Vec<_> = spans
            .into_iter()
            .map(|span| {
                let first = pixels[span.start];
                let values = &pixels[span.clone()];
                let fractions = if let Some(existing) = self.fractions.iter().find(|existing| {
                    existing.len() == values.len()
                        && existing.iter().zip(values).all(|(fraction, pixel)| {
                            fraction.to_bits() == pixel.pixel_fraction.to_bits()
                        })
                }) {
                    Shared::clone(existing)
                } else {
                    let fractions: Shared<[f32]> = values
                        .iter()
                        .map(|p| p.pixel_fraction)
                        .collect::<Vec<_>>()
                        .into();
                    self.fractions.push(Shared::clone(&fractions));
                    fractions
                };
                PixelRun {
                    end: span.end,
                    fixture: first.fixture_index,
                    first_cell: first.fixture_pixel_index,
                    first_index: first.pixel_index,
                    count: first.pixel_count,
                    fractions,
                }
            })
            .collect();
        if let Some(existing) = self.runs.iter().find(|existing| existing.as_ref() == runs) {
            return TargetPixels::Runs(Shared::clone(existing));
        }
        let runs: Shared<[PixelRun]> = runs.into();
        self.runs.push(Shared::clone(&runs));
        TargetPixels::Runs(runs)
    }
}
