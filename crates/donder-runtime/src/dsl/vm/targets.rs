use super::super::bytecode::TargetMember;
use super::{Arc, TargetItemValue, TargetItemsValue, TargetValue, clamp_array_index};
use crate::signal::PreparedPixel;
use alloc::{vec, vec::Vec};

/// Empty is the target type's ordinary empty value, with no shared allocation.
/// Registers otherwise retain the original target and its global pixel records.
#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(super) enum TargetRegister<T> {
    #[default]
    Empty,
    Shared(Arc<T>),
}

impl<T: Default> TargetRegister<T> {
    pub(super) fn owned(&self) -> Arc<T> {
        match self {
            Self::Empty => Arc::new(T::default()),
            Self::Shared(value) => Arc::clone(value),
        }
    }
}

impl TargetRegister<TargetValue> {
    pub(super) fn groups(&self) -> &[Arc<TargetItemValue>] {
        match self {
            Self::Empty => &[],
            Self::Shared(value) => &value.groups,
        }
    }
}

impl TargetRegister<TargetItemsValue> {
    pub(super) fn groups(&self) -> &[Arc<TargetItemValue>] {
        match self {
            Self::Empty => &[],
            Self::Shared(value) => &value.groups,
        }
    }
}

impl TargetRegister<TargetItemValue> {
    pub(super) fn pixels(&self) -> &[PreparedPixel] {
        match self {
            Self::Empty => &[],
            Self::Shared(value) => &value.pixels,
        }
    }

    pub(super) fn member(&self, member: TargetMember) -> i32 {
        self.pixels().first().map_or(0, |pixel| match member {
            TargetMember::FixtureIndex => pixel.fixture_index as i32,
            TargetMember::FixturePixelIndex => pixel.fixture_pixel_index as i32,
            TargetMember::PixelIndex => pixel.pixel_index as i32,
            TargetMember::PixelCount => pixel.pixel_count as i32,
        })
    }

    pub(super) fn fraction(&self) -> f32 {
        self.pixels()
            .first()
            .map_or(0.0, |pixel| pixel.pixel_fraction)
    }
}

pub(super) enum TargetView<'a> {
    Groups(&'a [Arc<TargetItemValue>]),
    Pixels(&'a [PreparedPixel]),
}

impl TargetView<'_> {
    fn for_each_pixel(self, mut visit: impl FnMut(PreparedPixel)) {
        match self {
            Self::Groups(groups) => {
                for group in groups {
                    for pixel in group.pixels.iter().copied() {
                        visit(pixel);
                    }
                }
            }
            Self::Pixels(pixels) => {
                for pixel in pixels.iter().copied() {
                    visit(pixel);
                }
            }
        }
    }
}

pub(super) fn pick(items: &[Arc<TargetItemValue>], index: i32) -> TargetRegister<TargetItemValue> {
    if items.is_empty() {
        TargetRegister::Empty
    } else {
        TargetRegister::Shared(Arc::clone(&items[clamp_array_index(index, items.len())]))
    }
}

pub(super) fn fixtures(value: TargetView<'_>) -> TargetItemsValue {
    regroup(value, |first, next| {
        first.fixture_index == next.fixture_index
    })
}

pub(super) fn pixels(value: TargetView<'_>) -> TargetItemsValue {
    let mut groups = Vec::new();
    value.for_each_pixel(|pixel| {
        groups.push(Arc::new(TargetItemValue {
            pixels: Arc::from([pixel]),
        }));
    });
    TargetItemsValue { groups }
}

pub(super) fn sections(value: TargetView<'_>, width: f32) -> TargetItemsValue {
    let width = libm::floorf(width.max(1.0)) as u32;
    regroup(value, |first, next| {
        first.fixture_index == next.fixture_index
            && first.fixture_pixel_index / width == next.fixture_pixel_index / width
    })
}

/// Regroup consecutive pixels only. Never renumber or rebuild their sampling context.
fn regroup(
    value: TargetView<'_>,
    same_group: impl Fn(&PreparedPixel, &PreparedPixel) -> bool,
) -> TargetItemsValue {
    let mut groups: Vec<Vec<PreparedPixel>> = Vec::new();
    value.for_each_pixel(|pixel| {
        if let Some(group) = groups.last_mut()
            && group.first().is_some_and(|first| same_group(first, &pixel))
        {
            group.push(pixel);
        } else {
            groups.push(vec![pixel]);
        }
    });
    TargetItemsValue {
        groups: groups
            .into_iter()
            .map(|pixels| {
                Arc::new(TargetItemValue {
                    pixels: pixels.into(),
                })
            })
            .collect(),
    }
}
