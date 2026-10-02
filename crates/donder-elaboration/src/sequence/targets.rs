use donder_language::dsl::{TargetItemValue, TargetValue};
use donder_language::effect::EffectScope;
use donder_language::layout::FixtureInstanceId;
use donder_language::layout::FixtureTarget;
pub(crate) use donder_runtime::signal::PreparedPixel as PreparedTargetPixel;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;

use crate::sequence::fixtures::PreparedFixture;

fn pixel_fraction(index: usize, count: usize) -> f32 {
    if count <= 1 {
        0.0
    } else {
        index as f32 / (count - 1) as f32
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct PreparedTargetSelection {
    pub(crate) fixtures: Vec<FixtureInstanceId>,
}

#[derive(Default)]
pub(crate) struct PreparedTargetCache {
    prepared_targets: HashMap<PreparedTargetCacheKey, Arc<[PreparedTargetPixel]>>,
    pub(crate) sample_targets: Vec<Arc<[PreparedTargetPixel]>>,
    generator_context_targets: HashMap<usize, GeneratorContextTargetCacheEntry>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PreparedTargetCacheKey {
    target: PreparedTargetSelection,
    scope: PreparedTargetScopeKey,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum PreparedTargetScopeKey {
    PerFixture,
    WholeTarget,
}

impl From<&EffectScope> for PreparedTargetScopeKey {
    fn from(scope: &EffectScope) -> Self {
        match scope {
            EffectScope::PerFixture => Self::PerFixture,
            EffectScope::WholeTarget => Self::WholeTarget,
        }
    }
}

pub(crate) fn full_rig_target_pixels(fixtures: &[PreparedFixture]) -> Vec<PreparedTargetPixel> {
    let mut pixels = Vec::new();
    for (fixture_index, fixture) in fixtures.iter().enumerate() {
        for fixture_pixel_index in 0..fixture.pixel_count {
            let pixel_index = fixture_pixel_index;
            let pixel_fraction = if fixture.pixel_count <= 1 {
                0.0
            } else {
                fixture_pixel_index as f32 / (fixture.pixel_count - 1) as f32
            };
            pixels.push(PreparedTargetPixel {
                fixture_index,
                fixture_pixel_index: fixture_pixel_index as u32,
                pixel_index,
                pixel_count: fixture.pixel_count,
                pixel_fraction,
            });
        }
    }
    pixels
}

pub(crate) fn prepare_target(
    target: &FixtureTarget,
    groups: &IndexMap<FixtureInstanceId, Vec<FixtureInstanceId>>,
) -> PreparedTargetSelection {
    if let Some(members) = groups.get(&target.fixture) {
        return PreparedTargetSelection {
            fixtures: members.clone(),
        };
    }
    PreparedTargetSelection {
        fixtures: vec![target.fixture],
    }
}

fn prepare_target_indexes(
    target: &[FixtureInstanceId],
    fixtures: &[PreparedFixture],
) -> Vec<usize> {
    let indexes = fixtures
        .iter()
        .enumerate()
        .map(|(index, fixture)| (fixture.id, index))
        .collect::<IndexMap<_, _>>();
    target.iter().map(|id| indexes[id]).collect()
}

pub(crate) fn prepare_target_pixels(
    target: &PreparedTargetSelection,
    fixtures: &[PreparedFixture],
    scope: &EffectScope,
) -> Vec<PreparedTargetPixel> {
    let indexes = prepare_target_indexes(&target.fixtures, fixtures);
    let total_target_pixels = indexes
        .iter()
        .map(|index| fixtures[*index].pixel_count)
        .sum();
    let mut pixels = Vec::with_capacity(total_target_pixels);
    let mut whole_index = 0usize;
    for fixture_index in indexes {
        let fixture_pixel_count = fixtures[fixture_index].pixel_count;
        for fixture_pixel_index in 0..fixture_pixel_count {
            let (pixel_index, pixel_count) = match scope {
                EffectScope::PerFixture => (fixture_pixel_index, fixture_pixel_count),
                EffectScope::WholeTarget => (whole_index, total_target_pixels),
            };
            pixels.push(PreparedTargetPixel {
                fixture_index,
                fixture_pixel_index: fixture_pixel_index as u32,
                pixel_index,
                pixel_count,
                pixel_fraction: pixel_fraction(pixel_index, pixel_count),
            });
            whole_index += 1;
        }
    }
    pixels
}

pub(crate) fn prepare_target_pixels_cached(
    cache: &mut PreparedTargetCache,
    target: &PreparedTargetSelection,
    fixtures: &[PreparedFixture],
    scope: &EffectScope,
) -> Arc<[PreparedTargetPixel]> {
    let key = PreparedTargetCacheKey {
        target: target.clone(),
        scope: PreparedTargetScopeKey::from(scope),
    };
    if let Some(pixels) = cache.prepared_targets.get(&key) {
        return Arc::clone(pixels);
    }
    let pixels = Arc::from(prepare_target_pixels(target, fixtures, scope));
    cache.prepared_targets.insert(key, Arc::clone(&pixels));
    pixels
}

pub(crate) fn sorted_sample_target(
    target: &Arc<[PreparedTargetPixel]>,
) -> Arc<[PreparedTargetPixel]> {
    if target.is_sorted_by_key(|pixel| (pixel.fixture_index, pixel.fixture_pixel_index)) {
        return Arc::clone(target);
    }
    let mut sorted = target.to_vec();
    sorted.sort_by_key(|pixel| (pixel.fixture_index, pixel.fixture_pixel_index));
    Arc::from(sorted)
}

pub(crate) fn generator_expansion_targets(
    target: &Arc<[PreparedTargetPixel]>,
    scope: &EffectScope,
) -> Vec<Arc<[PreparedTargetPixel]>> {
    match scope {
        EffectScope::WholeTarget => vec![Arc::clone(target)],
        EffectScope::PerFixture => {
            let mut targets = Vec::new();
            let mut fixture_pixels = Vec::new();
            let mut current_fixture_index = None;

            for pixel in target.iter() {
                if current_fixture_index.is_some_and(|index| index != pixel.fixture_index) {
                    targets.push(Arc::from(fixture_pixels));
                    fixture_pixels = Vec::new();
                }
                current_fixture_index = Some(pixel.fixture_index);
                fixture_pixels.push(pixel.clone());
            }

            if !fixture_pixels.is_empty() {
                targets.push(Arc::from(fixture_pixels));
            }

            targets
        }
    }
}

struct GeneratorContextTargetCacheEntry {
    source: Arc<[PreparedTargetPixel]>,
    target: Arc<TargetValue>,
}

fn arc_key<T: ?Sized>(value: &Arc<T>) -> usize {
    Arc::as_ptr(value).cast::<()>() as usize
}

fn target_groups_from_pixels(pixels: &Arc<[PreparedTargetPixel]>) -> Vec<Arc<TargetItemValue>> {
    vec![Arc::new(TargetItemValue {
        pixels: Arc::clone(pixels),
    })]
}

pub(crate) fn generator_context_target(
    cache: &mut PreparedTargetCache,
    prepared_target: &Arc<[PreparedTargetPixel]>,
) -> Arc<TargetValue> {
    let key = arc_key(prepared_target);
    if let Some(entry) = cache.generator_context_targets.get(&key)
        && Arc::ptr_eq(&entry.source, prepared_target)
    {
        return Arc::clone(&entry.target);
    }
    let target = Arc::new(TargetValue {
        groups: target_groups_from_pixels(prepared_target),
    });
    cache.generator_context_targets.insert(
        key,
        GeneratorContextTargetCacheEntry {
            source: Arc::clone(prepared_target),
            target: Arc::clone(&target),
        },
    );
    target
}

impl PreparedTargetCache {
    pub(crate) fn sample_target(&mut self, pixels: Arc<[PreparedTargetPixel]>) -> usize {
        let key = |pixel: &PreparedTargetPixel| {
            (
                pixel.fixture_index,
                pixel.fixture_pixel_index,
                pixel.pixel_index,
                pixel.pixel_count,
                pixel.pixel_fraction.to_bits(),
            )
        };
        // Intern exact contexts, including local indices and float bits, not just output addresses.
        let index = self
            .sample_targets
            .iter()
            .position(|target| {
                Arc::ptr_eq(target, &pixels) || target.iter().map(key).eq(pixels.iter().map(key))
            })
            .unwrap_or(self.sample_targets.len());
        if index == self.sample_targets.len() {
            self.sample_targets.push(pixels);
        }
        index
    }
}

/// Preserve the effect's original spatial scope before output fragmentation or raster sampling.
pub(crate) fn spatial_contexts(
    pixels: &[PreparedTargetPixel],
    fixtures: &[PreparedFixture],
) -> Vec<donder_runtime::dsl::SpatialContext> {
    let whole_target = pixels
        .iter()
        .all(|pixel| pixel.pixel_count() == pixels.len());
    let mut bounds = HashMap::<Option<usize>, ([f32; 2], [f32; 2])>::new();
    for pixel in pixels {
        let position = fixtures[pixel.fixture_index()].positions[pixel.fixture_pixel_index()];
        let key = (!whole_target).then_some(pixel.fixture_index());
        let (min, max) = bounds.entry(key).or_insert((position, position));
        for axis in 0..2 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
    }
    pixels
        .iter()
        .map(|pixel| {
            let (min, max) = bounds[&(!whole_target).then_some(pixel.fixture_index())];
            donder_runtime::dsl::SpatialContext {
                position: fixtures[pixel.fixture_index()].positions[pixel.fixture_pixel_index()],
                min,
                max,
            }
        })
        .collect()
}

#[cfg(test)]
mod representation_tests {
    use super::{PreparedFixture, PreparedTargetPixel, full_rig_target_pixels};
    use donder_language::layout::FixtureInstanceId;

    #[test]
    fn prepared_target_pixel_uses_native_addresses_and_u32_fixture_cells() {
        let pixel = PreparedTargetPixel {
            fixture_index: usize::MAX,
            fixture_pixel_index: u32::MAX,
            pixel_index: usize::MAX,
            pixel_count: usize::MAX,
            pixel_fraction: 1.0,
        };
        assert_eq!(pixel.fixture_index(), usize::MAX);
        assert_eq!(pixel.fixture_pixel_index(), u32::MAX as usize);
        assert_eq!(pixel.pixel_index(), usize::MAX);
        assert_eq!(pixel.pixel_count(), usize::MAX);
    }

    #[test]
    fn full_rig_target_accepts_a_fixture_larger_than_u16_indices() {
        let pixels = full_rig_target_pixels(&[PreparedFixture {
            id: FixtureInstanceId(1),
            pixel_count: 65_537,
            positions: Vec::new(),
        }]);

        assert_eq!(pixels.len(), 65_537);
        assert_eq!(pixels.last().unwrap().fixture_pixel_index, 65_536);
    }
}
