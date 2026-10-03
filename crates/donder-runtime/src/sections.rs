//! Original target membership for section queries. Routing may discard physical
//! pixels, but must not change the authored section population or numbering.
use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};

pub(crate) fn normalize_width(width: f32) -> u32 {
    libm::floorf(width.max(1.0)) as u32
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub(crate) struct SectionPixel {
    run: usize,
    index: u32,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct SectionRun {
    fixture: usize,
    first: u32,
    last: u32,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct SectionSpan {
    end: usize,
    first: SectionPixel,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
enum SectionPixels {
    Runs(Box<[SectionSpan]>),
    Indexed(Box<[SectionPixel]>),
}

impl Default for SectionPixels {
    fn default() -> Self {
        Self::Runs(Box::new([]))
    }
}

#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedSections {
    runs: Box<[SectionRun]>,
    pixels: SectionPixels,
    per_fixture: bool,
}

impl PreparedSections {
    pub(crate) fn new(
        selection: &[(usize, u32)],
        pixels: &[(usize, u32)],
        per_fixture: bool,
    ) -> Self {
        let mut runs = Vec::<SectionRun>::new();
        let mut addresses = BTreeMap::new();
        for &(fixture, index) in selection {
            if let Some(run) = runs.last_mut()
                && run.fixture == fixture
                && run.last.checked_add(1) == Some(index)
            {
                run.last = index;
            } else {
                runs.push(SectionRun {
                    fixture,
                    first: index,
                    last: index,
                });
            }
            addresses.insert(
                (fixture, index),
                SectionPixel {
                    run: runs.len() - 1,
                    index,
                },
            );
        }
        let mut spans = Vec::<SectionSpan>::new();
        for (index, pixel) in pixels.iter().enumerate() {
            let address = addresses[pixel];
            let start = if spans.len() > 1 {
                spans[spans.len() - 2].end
            } else {
                0
            };
            if let Some(span) = spans.last_mut()
                && span.first.run == address.run
                && span.first.index.checked_add((index - start) as u32) == Some(address.index)
            {
                span.end = index + 1;
            } else {
                spans.push(SectionSpan {
                    end: index + 1,
                    first: address,
                });
            }
        }
        let pixels = if spans.len() * core::mem::size_of::<rkyv::Archived<SectionSpan>>()
            < pixels.len() * core::mem::size_of::<rkyv::Archived<SectionPixel>>()
        {
            SectionPixels::Runs(spans.into())
        } else {
            SectionPixels::Indexed(pixels.iter().map(|pixel| addresses[pixel]).collect())
        };
        Self {
            runs: runs.into(),
            pixels,
            per_fixture,
        }
    }

    pub(crate) fn pixel(&self, index: usize) -> SectionPixel {
        match &self.pixels {
            // Programs without section queries do not allocate this metadata.
            SectionPixels::Runs(spans) if spans.is_empty() => SectionPixel::default(),
            SectionPixels::Indexed(pixels) if pixels.is_empty() => SectionPixel::default(),
            SectionPixels::Indexed(pixels) => pixels[index],
            SectionPixels::Runs(spans) => {
                let span = spans.partition_point(|span| span.end <= index);
                let start = if span == 0 { 0 } else { spans[span - 1].end };
                let first = spans[span].first;
                SectionPixel {
                    run: first.run,
                    index: first.index + (index - start) as u32,
                }
            }
        }
    }

    fn query(&self, pixel: SectionPixel, width: i32, index: bool) -> i32 {
        let Some(current) = self.runs.get(pixel.run) else {
            return if index { -1 } else { 0 };
        };
        let width = normalize_width(width as f32);
        let mut count = 0_u64;
        let mut selected = 0_u64;
        let mut previous = None;
        for (run_index, run) in self.runs.iter().enumerate() {
            if self.per_fixture && run.fixture != current.fixture {
                continue;
            }
            let first = run.first / width;
            let last = run.last / width;
            let merge = u64::from(previous == Some((run.fixture, first)));
            if run_index == pixel.run {
                selected = count - merge + u64::from(pixel.index / width - first);
            }
            count += u64::from(last - first) + 1 - merge;
            previous = Some((run.fixture, last));
        }
        (if index { selected } else { count }).min(i32::MAX as u64) as i32
    }
}

/// Standalone evaluation has one virtual fixture described by RunContext.
/// Prepared playback supplies the original fixture runs instead.
#[derive(Clone, Copy)]
pub(crate) enum SectionContext<'a> {
    Single {
        index: i32,
        count: i32,
    },
    Prepared {
        target: &'a PreparedSections,
        pixel: SectionPixel,
    },
}

impl SectionContext<'_> {
    pub(crate) fn query(self, width: i32, index: bool) -> i32 {
        match self {
            Self::Prepared { target, pixel } => target.query(pixel, width, index),
            Self::Single {
                index: pixel,
                count,
            } => {
                if count <= 0 {
                    return if index { -1 } else { 0 };
                }
                let width = normalize_width(width as f32);
                if index {
                    (pixel.clamp(0, count - 1) as u32 / width) as i32
                } else {
                    (1 + (count as u32 - 1) / width) as i32
                }
            }
        }
    }
}
