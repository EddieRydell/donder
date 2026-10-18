//! Original target membership for section queries. Routing may discard
//! physical pixels, but must not change the authored section population or
//! numbering.
use donder_runtime_types::image::{self, SectionPixel};

use crate::show::Show;

pub(crate) fn normalize_width(width: f32) -> u32 {
    libm::floorf(width.max(1.0)) as u32
}

/// A target's section runs, and each of its pixels' place among them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SectionsView<'a> {
    runs: &'a [image::SectionRun],
    spans: &'a [image::SectionSpan],
    pixels: &'a [SectionPixel],
    per_fixture: bool,
}

impl<'a> SectionsView<'a> {
    #[inline(never)]
    pub(crate) fn of(show: Show<'a>, target: &'a image::Target) -> Self {
        let s = &show.s;
        let (spans, pixels) = if target.section_kind == image::PIXELS_RUNS {
            (target.section_pixels.get(s.section_spans), &[][..])
        } else {
            (&[][..], target.section_pixels.get(s.section_pixels))
        };
        Self {
            runs: target.section_runs.get(s.section_runs),
            spans,
            pixels,
            per_fixture: target.per_fixture != 0,
        }
    }

    /// Target pixel `index`'s place; targets of programs without section
    /// queries hold none.
    pub(crate) fn pixel(&self, index: usize) -> SectionPixel {
        if !self.pixels.is_empty() {
            return self.pixels[index];
        }
        if self.spans.is_empty() {
            return SectionPixel::default();
        }
        let span = self
            .spans
            .partition_point(|span| span.end as usize <= index);
        let start = if span == 0 {
            0
        } else {
            self.spans[span - 1].end as usize
        };
        let first = self.spans[span].first;
        SectionPixel {
            run: first.run,
            index: first.index + (index - start) as u32,
        }
    }

    /// The sections of the pixels in `pixel`'s run at `width`. Every pixel of
    /// a run shares the result, so a strip computes it once per run.
    pub(crate) fn run_sections(&self, pixel: SectionPixel, width: i32) -> RunSections {
        let key = (pixel.run, width);
        let Some(current) = self.runs.get(pixel.run as usize) else {
            return RunSections {
                key,
                width: 1,
                base: -1,
                count: 0,
                missing: true,
            };
        };
        let width = normalize_width(width as f32);
        let mut count = 0_u32;
        let mut base = 0_i32;
        let mut previous = None;
        for (run_index, run) in self.runs.iter().enumerate() {
            if self.per_fixture && run.fixture != current.fixture {
                continue;
            }
            let first = run.first / width;
            let last = run.last / width;
            let merge = u32::from(previous == Some((run.fixture, first)));
            if run_index == pixel.run as usize {
                base = (count - merge) as i32 - first as i32;
            }
            count += last - first + 1 - merge;
            previous = Some((run.fixture, last));
        }
        RunSections {
            key,
            width,
            base,
            count: count.min(i32::MAX as u32) as i32,
            missing: false,
        }
    }
}

/// One run's section numbering at one width.
#[derive(Clone, Copy)]
pub(crate) struct RunSections {
    /// The run and the requested width it answers.
    pub(crate) key: (u32, i32),
    width: u32,
    /// A pixel's section index is `base + pixel.index / width`.
    base: i32,
    count: i32,
    missing: bool,
}

impl RunSections {
    #[inline(always)]
    pub(crate) fn query(&self, pixel: SectionPixel, index: bool) -> i32 {
        if !index {
            return self.count;
        }
        if self.missing {
            return -1;
        }
        self.base + (pixel.index / self.width) as i32
    }
}

/// Sections of standalone evaluation, which has one virtual fixture of
/// `count` pixels; prepared playback uses [`RunSections`] instead.
pub(crate) fn single_section_query(pixel: i32, count: i32, width: i32, index: bool) -> i32 {
    if count <= 0 {
        return if index { -1 } else { 0 };
    }
    let width = normalize_width(width as f32);
    if index {
        (pixel as u32 / width) as i32
    } else {
        (1 + (count as u32 - 1) / width) as i32
    }
}
