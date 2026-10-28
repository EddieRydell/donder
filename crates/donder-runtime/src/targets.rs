//! Targets as an image stores them: runs of contiguous cells, or one record
//! per pixel for irregular selections, with each pixel's original logical
//! place. A pixel's fraction is computed from its place, as authoring does.
use crate::image::{self, PixelRun};
use core::cell::Cell;
use donder_runtime_types::SpatialContext;

use crate::sections::SectionsView;
use crate::show::Show;

/// A target pixel: its storage fixture and cell, and its index among the
/// `pixel_count` pixels of its sampling scope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PreparedPixel {
    pub fixture_index: usize,
    pub fixture_pixel_index: u32,
    pub pixel_index: usize,
    pub pixel_count: usize,
}

impl PreparedPixel {
    /// 0 at the scope's first pixel, 1 at its last; computed as
    /// `TargetPixel::fraction` computes it, bit for bit.
    #[inline(always)]
    pub(crate) fn fraction(&self) -> f32 {
        if self.pixel_count <= 1 {
            0.0
        } else {
            self.pixel_index as f32 / (self.pixel_count - 1) as f32
        }
    }
}

#[inline(always)]
fn run_pixel(run: &PixelRun, offset: usize) -> PreparedPixel {
    PreparedPixel {
        fixture_index: run.fixture as usize,
        fixture_pixel_index: run.first_cell + offset as u32,
        pixel_index: run.first_index as usize + offset,
        pixel_count: run.count as usize,
    }
}

#[inline(always)]
fn indexed_pixel(pixel: &image::Pixel) -> PreparedPixel {
    PreparedPixel {
        fixture_index: pixel.fixture as usize,
        fixture_pixel_index: pixel.cell,
        pixel_index: pixel.index as usize,
        pixel_count: pixel.count as usize,
    }
}

/// One target of a show.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TargetView<'a> {
    pub(crate) record: &'a image::Target,
    runs: &'a [PixelRun],
    pixels: &'a [image::Pixel],
    spatial: &'a [image::SpatialRun],
    pub(crate) sections: SectionsView<'a>,
}

/// Consecutive target pixels that share a fixture, count and contiguous
/// physical cells. Indexed targets have one pixel per segment.
pub(crate) struct Segment {
    /// Target index of the first pixel.
    pub start: usize,
    pub fixture: usize,
    pub first_cell: u32,
    pub first_index: usize,
    pub count: usize,
    pub len: usize,
}

impl<'a> TargetView<'a> {
    // Once per effect sample, so it stays in flash.
    #[inline(never)]
    pub(crate) fn of(show: Show<'a>, record: &'a image::Target) -> Self {
        let s = &show.s;
        let (runs, pixels) = if record.kind == image::PIXELS_RUNS {
            (record.pixels.get(s.pixel_runs), &[][..])
        } else {
            (&[][..], record.pixels.get(s.pixels))
        };
        Self {
            record,
            runs,
            pixels,
            spatial: record.spatial.get(s.spatial_runs),
            sections: SectionsView::of(show, record),
        }
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        if self.record.kind == image::PIXELS_RUNS {
            self.runs.last().map_or(0, |run| run.end as usize)
        } else {
            self.pixels.len()
        }
    }

    pub(crate) fn sample_count(&self) -> usize {
        self.record.sample_count as usize
    }

    fn run_start(&self, run: usize) -> usize {
        if run == 0 {
            0
        } else {
            self.runs[run - 1].end as usize
        }
    }

    #[inline]
    pub(crate) fn pixel(&self, index: usize) -> PreparedPixel {
        if self.record.kind != image::PIXELS_RUNS {
            return indexed_pixel(&self.pixels[index]);
        }
        if let [run] = self.runs {
            return run_pixel(run, index);
        }
        let run = self.runs.partition_point(|run| run.end as usize <= index);
        run_pixel(&self.runs[run], index - self.run_start(run))
    }

    #[inline]
    pub(crate) fn segment_count(&self) -> usize {
        if self.record.kind == image::PIXELS_RUNS {
            self.runs.len()
        } else {
            self.pixels.len()
        }
    }

    #[inline]
    pub(crate) fn segment(&self, index: usize) -> Segment {
        if self.record.kind != image::PIXELS_RUNS {
            let pixel = &self.pixels[index];
            return Segment {
                start: index,
                fixture: pixel.fixture as usize,
                first_cell: pixel.cell,
                first_index: pixel.index as usize,
                count: pixel.count as usize,
                len: 1,
            };
        }
        let run = &self.runs[index];
        let start = self.run_start(index);
        Segment {
            start,
            fixture: run.fixture as usize,
            first_cell: run.first_cell,
            first_index: run.first_index as usize,
            count: run.count as usize,
            len: run.end as usize - start,
        }
    }

    pub(crate) fn segments(&self) -> impl Iterator<Item = Segment> + '_ {
        (0..self.segment_count()).map(|index| self.segment(index))
    }

    /// Pixel lookups that remember the last pixel's run, for callers that
    /// visit target indices in ascending order.
    pub(crate) fn cursor(&self) -> TargetCursor<'a> {
        TargetCursor {
            target: *self,
            run: Cell::new(0),
            start: Cell::new(0),
        }
    }

    /// The pixels from target index `index` on.
    pub(crate) fn iter_from(&self, index: usize) -> impl Iterator<Item = PreparedPixel> + 'a {
        let cursor = self.cursor();
        (index..self.len()).map(move |index| cursor.pixel(index))
    }

    pub(crate) fn spatial_context(
        &self,
        index: usize,
        pixel: &PreparedPixel,
        show: Show<'a>,
    ) -> SpatialContext {
        let run = &self.spatial[self
            .spatial
            .partition_point(|run| run.end as usize <= index)];
        let fixture = &show.s.fixtures[pixel.fixture_index];
        let position = &fixture.positions.get(show.s.positions)[pixel.fixture_pixel_index as usize];
        SpatialContext {
            position: [position.x, position.y],
            min: run.min,
            max: run.max,
        }
    }
}

pub(crate) struct TargetCursor<'a> {
    target: TargetView<'a>,
    run: Cell<usize>,
    start: Cell<usize>,
}

impl TargetCursor<'_> {
    #[inline]
    pub(crate) fn pixel(&self, index: usize) -> PreparedPixel {
        let target = &self.target;
        if target.record.kind != image::PIXELS_RUNS {
            return indexed_pixel(&target.pixels[index]);
        }
        let runs = target.runs;
        let (mut run, mut start) = (self.run.get(), self.start.get());
        if index < start || index >= runs[run].end as usize {
            if index == runs[run].end as usize {
                (run, start) = (run + 1, index);
            } else {
                run = runs.partition_point(|run| run.end as usize <= index);
                start = target.run_start(run);
            }
            self.run.set(run);
            self.start.set(start);
        }
        run_pixel(&runs[run], index - start)
    }
}
