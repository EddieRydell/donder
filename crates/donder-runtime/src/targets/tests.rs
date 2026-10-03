use super::*;
use alloc::vec;

fn pixel(fixture: usize, cell: u32, index: usize, count: usize) -> PreparedPixel {
    PreparedPixel {
        fixture_index: fixture,
        fixture_pixel_index: cell,
        pixel_index: index,
        pixel_count: count,
        pixel_fraction: index as f32 / (count - 1).max(1) as f32,
    }
}

fn assert_mapping(actual: &TargetPixels, expected: &[PreparedPixel]) {
    assert_eq!(actual.len(), expected.len());
    assert_eq!(actual.iter().collect::<Vec<_>>(), expected);
    for (index, expected) in expected.iter().enumerate() {
        assert_eq!(actual.pixel(index), *expected);
        assert_eq!(
            actual.pixel(index).pixel_fraction.to_bits(),
            expected.pixel_fraction.to_bits()
        );
        assert_eq!(
            actual.find(expected.fixture_index, expected.fixture_pixel_index),
            Some(index)
        );
    }
    assert_eq!(actual.find(usize::MAX, 0), None);
    let mut iter = actual.iter();
    for remaining in (1..=expected.len()).rev() {
        assert_eq!(iter.len(), remaining);
        assert!(iter.next().is_some());
    }
    assert_eq!(iter.len(), 0);
    assert!(iter.next().is_none());
    assert!(iter.next().is_none());
}

#[test]
fn runs_preserve_disjoint_storage_and_reordered_logical_domains() {
    let expected: Vec<_> = (0..48)
        .map(|cell| pixel(0, cell, cell as usize + 70, 150))
        .chain((48..96).map(|cell| pixel(0, cell, cell as usize + 100, 300)))
        .chain((0..48).map(|cell| pixel(1, cell, cell as usize, 48)))
        .collect();
    let mut interner = TargetInterner::default();
    let actual = interner.prepare(expected.clone());
    assert!(matches!(actual, TargetPixels::Runs(_)));
    assert_mapping(&actual, &expected);
    assert_eq!(actual.find(0, 96), None);
    assert_eq!(actual.find(1, 48), None);
    assert_eq!(actual.find(2, 0), None);
    let repeated = interner.prepare(expected);
    let (TargetPixels::Runs(a), TargetPixels::Runs(b)) = (&actual, &repeated) else {
        panic!()
    };
    assert!(Shared::ptr_eq(a, b));
}

#[test]
fn irregular_selections_choose_indexed_storage_and_keep_every_context() {
    let expected: Vec<_> = (0..64)
        .map(|cell| pixel(0, cell * 3, (63 - cell) as usize, 64))
        .collect();
    let mut interner = TargetInterner::default();
    let actual = interner.prepare(expected.clone());
    assert!(matches!(actual, TargetPixels::Indexed(_)));
    assert_mapping(&actual, &expected);
    assert_eq!(actual.find(0, 1), None);
    let repeated = interner.prepare(expected);
    let (TargetPixels::Indexed(a), TargetPixels::Indexed(b)) = (&actual, &repeated) else {
        panic!()
    };
    assert!(Shared::ptr_eq(a, b));
    assert_mapping(&interner.prepare(vec![]), &[]);
}

#[test]
fn fractions_share_storage_across_fixtures_without_changing_float_bits() {
    let mut interner = TargetInterner::default();
    let first = interner.prepare(
        (0..150)
            .map(|cell| pixel(0, cell, cell as usize, 150))
            .collect(),
    );
    let second = interner.prepare(
        (0..150)
            .map(|cell| pixel(1, cell, cell as usize, 150))
            .collect(),
    );
    let (TargetPixels::Runs(a), TargetPixels::Runs(b)) = (&first, &second) else {
        panic!()
    };
    assert!(Shared::ptr_eq(&a[0].fractions, &b[0].fractions));
    for index in 0..150 {
        assert_eq!(
            first.pixel(index).pixel_fraction.to_bits(),
            (index as f32 / 149.0).to_bits()
        );
    }
}
