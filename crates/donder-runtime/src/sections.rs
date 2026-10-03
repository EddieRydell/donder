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

#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedSections {
    runs: Box<[SectionRun]>,
    pub(crate) pixels: Box<[SectionPixel]>,
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
        Self {
            runs: runs.into(),
            pixels: pixels.iter().map(|pixel| addresses[pixel]).collect(),
            per_fixture,
        }
    }

    pub(crate) fn pixel(&self, index: usize) -> SectionPixel {
        if self.pixels.is_empty() {
            // Programs without section queries do not allocate this metadata.
            SectionPixel::default()
        } else {
            self.pixels[index]
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
