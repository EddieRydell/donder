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
    let width = crate::sections::normalize_width(width);
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

#[cfg(test)]
mod section_query_tests {
    use super::*;
    use crate::sections::{PreparedSections, SectionContext};

    #[test]
    fn section_queries_match_generator_groups_for_ordered_sparse_membership() {
        let selection: Vec<_> = [
            (4, 3),
            (4, 4),
            (4, 7),
            (4, 8),
            (1, 2),
            (1, 3),
            (1, 9),
            (4, 12),
        ]
        .into_iter()
        .enumerate()
        .map(|(logical, (fixture, cell))| {
            PreparedPixel::try_new(fixture, cell, logical, 8, logical as f32 / 7.0).unwrap()
        })
        .collect();
        let mut physical = selection.clone();
        physical.sort_by_key(|pixel| (pixel.fixture_index, pixel.fixture_pixel_index));
        for width in [-1, 0, 1, 3, 5, 20, i32::MAX] {
            for per_fixture in [false, true] {
                let prepared = PreparedSections::new(&selection, &physical, per_fixture);
                for (local, pixel) in physical.iter().enumerate() {
                    let scope: Vec<_> = selection
                        .iter()
                        .filter(|other| !per_fixture || other.fixture_index == pixel.fixture_index)
                        .copied()
                        .collect();
                    let generated = sections(TargetView::Pixels(&scope), width as f32);
                    let index = generated
                        .groups
                        .iter()
                        .position(|group| {
                            group.pixels.iter().any(|other| {
                                other.fixture_index == pixel.fixture_index
                                    && other.fixture_pixel_index == pixel.fixture_pixel_index
                            })
                        })
                        .unwrap();
                    let context = SectionContext::Prepared {
                        target: &prepared,
                        pixel: prepared.pixel(local),
                    };
                    assert_eq!(context.query(width, false), generated.groups.len() as i32);
                    assert_eq!(context.query(width, true), index as i32);
                }
            }
        }
    }
}
