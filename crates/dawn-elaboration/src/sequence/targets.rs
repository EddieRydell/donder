use dawn_language::dsl::{TargetItemValue, TargetPixelValue, TargetValue};
use dawn_language::effect::EffectScope;
use dawn_language::layout::FixtureInstanceId;
use dawn_language::layout::FixtureTarget;
use dawn_language::model::DawnProject;
use dawn_language::setup::SetupId;
pub(crate) use dawn_runtime::signal::PreparedPixel as PreparedTargetPixel;
use indexmap::{IndexMap, IndexSet};
use std::collections::HashMap;
use std::sync::Arc;

use crate::RenderError;
use crate::sequence::fixtures::{PreparedFixture, prepare_fixtures};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedTargetPixelAddress {
    pub fixture_id: FixtureInstanceId,
    pub fixture_pixel_index: usize,
}

fn pixel_fraction(index: usize, count: usize) -> f32 {
    if count <= 1 {
        0.0
    } else {
        index as f32 / (count - 1) as f32
    }
}

pub fn resolve_effect_target_pixel_addresses(
    project: &DawnProject,
    setup_id: &SetupId,
    target: &FixtureTarget,
    scope: &EffectScope,
) -> Result<Vec<RenderedTargetPixelAddress>, RenderError> {
    let setup = project
        .setups
        .get(setup_id)
        .ok_or_else(|| RenderError::MissingSetup {
            setup_id: setup_id.clone(),
        })?;
    let layout = project
        .layouts
        .get(&setup.layout)
        .ok_or(RenderError::MissingLayout)?;
    if target.layout != layout.id {
        return Err(RenderError::BadTarget);
    }
    let (fixtures, groups) = prepare_fixtures(project, layout)?;
    let fixture_ids = fixtures
        .iter()
        .map(|fixture| fixture.id)
        .collect::<IndexSet<_>>();
    let target = prepare_target(target, &fixture_ids, &groups)?;
    let pixels = prepare_target_pixels(&target, &fixtures, scope)?;
    Ok(pixels
        .into_iter()
        .map(|pixel| RenderedTargetPixelAddress {
            fixture_id: fixtures[pixel.fixture_index()].id,
            fixture_pixel_index: pixel.fixture_pixel_index(),
        })
        .collect())
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct PreparedTargetSelection {
    pub(crate) fixtures: Vec<FixtureInstanceId>,
}

#[derive(Default)]
pub(crate) struct PreparedTargetCache {
    prepared_targets: HashMap<PreparedTargetCacheKey, Arc<[PreparedTargetPixel]>>,
    pub(crate) sample_targets: Vec<Arc<[PreparedTargetPixel]>>,
    generated_targets: HashMap<usize, GeneratedTargetCacheEntry>,
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

pub(crate) fn full_rig_target_pixels(
    fixtures: &[PreparedFixture],
) -> Result<Vec<PreparedTargetPixel>, RenderError> {
    let mut pixels = Vec::new();
    for (fixture_index, fixture) in fixtures.iter().enumerate() {
        for fixture_pixel_index in 0..fixture.pixel_count {
            let pixel_index = fixture_pixel_index;
            let pixel_fraction = if fixture.pixel_count <= 1 {
                0.0
            } else {
                fixture_pixel_index as f32 / (fixture.pixel_count - 1) as f32
            };
            pixels.push(
                PreparedTargetPixel::try_new(
                    fixture_index,
                    fixture_pixel_index,
                    pixel_index,
                    fixture.pixel_count,
                    pixel_fraction,
                )
                .ok_or(RenderError::BadTarget)?,
            );
        }
    }
    Ok(pixels)
}

pub(crate) fn prepare_target(
    target: &FixtureTarget,
    fixture_ids: &IndexSet<FixtureInstanceId>,
    groups: &IndexMap<FixtureInstanceId, Vec<FixtureInstanceId>>,
) -> Result<PreparedTargetSelection, RenderError> {
    if let Some(members) = groups.get(&target.fixture) {
        return Ok(PreparedTargetSelection {
            fixtures: members.clone(),
        });
    }
    if !fixture_ids.contains(&target.fixture) {
        return Err(RenderError::MissingFixture {
            fixture_id: target.fixture,
        });
    }
    Ok(PreparedTargetSelection {
        fixtures: vec![target.fixture],
    })
}

fn prepare_target_indexes(
    target: &[FixtureInstanceId],
    fixtures: &[PreparedFixture],
) -> Result<Vec<usize>, RenderError> {
    target
        .iter()
        .map(|id| {
            fixtures
                .iter()
                .position(|fixture| &fixture.id == id)
                .ok_or(RenderError::MissingFixture { fixture_id: *id })
        })
        .collect()
}

pub(crate) fn prepare_target_pixels(
    target: &PreparedTargetSelection,
    fixtures: &[PreparedFixture],
    scope: &EffectScope,
) -> Result<Vec<PreparedTargetPixel>, RenderError> {
    let indexes = prepare_target_indexes(&target.fixtures, fixtures)?;
    let total_target_pixels = indexes.iter().try_fold(0usize, |total, index| {
        total
            .checked_add(fixtures[*index].pixel_count)
            .ok_or(RenderError::BadTarget)
    })?;
    let mut pixels = Vec::with_capacity(total_target_pixels);
    let mut whole_index = 0usize;
    for fixture_index in indexes {
        let fixture_pixel_count = fixtures[fixture_index].pixel_count;
        for fixture_pixel_index in 0..fixture_pixel_count {
            let (pixel_index, pixel_count) = match scope {
                EffectScope::PerFixture => (fixture_pixel_index, fixture_pixel_count),
                EffectScope::WholeTarget => (whole_index, total_target_pixels),
            };
            pixels.push(
                PreparedTargetPixel::try_new(
                    fixture_index,
                    fixture_pixel_index,
                    pixel_index,
                    pixel_count,
                    pixel_fraction(pixel_index, pixel_count),
                )
                .ok_or(RenderError::BadTarget)?,
            );
            whole_index += 1;
        }
    }
    Ok(pixels)
}

pub(crate) fn prepare_target_pixels_cached(
    cache: &mut PreparedTargetCache,
    target: &PreparedTargetSelection,
    fixtures: &[PreparedFixture],
    scope: &EffectScope,
) -> Result<Arc<[PreparedTargetPixel]>, RenderError> {
    let key = PreparedTargetCacheKey {
        target: target.clone(),
        scope: PreparedTargetScopeKey::from(scope),
    };
    if let Some(pixels) = cache.prepared_targets.get(&key) {
        return Ok(Arc::clone(pixels));
    }
    let pixels = Arc::from(prepare_target_pixels(target, fixtures, scope)?);
    cache.prepared_targets.insert(key, Arc::clone(&pixels));
    Ok(pixels)
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

struct GeneratedTargetCacheEntry {
    source: Arc<TargetItemValue>,
    pixels: Arc<[PreparedTargetPixel]>,
}

struct GeneratorContextTargetCacheEntry {
    source: Arc<[PreparedTargetPixel]>,
    target: Arc<TargetValue>,
}

fn arc_key<T: ?Sized>(value: &Arc<T>) -> usize {
    Arc::as_ptr(value).cast::<()>() as usize
}

fn target_groups_from_pixels(pixels: &[PreparedTargetPixel]) -> Vec<Arc<TargetItemValue>> {
    vec![Arc::new(TargetItemValue {
        pixels: Arc::from(pixels.iter().map(target_pixel_value).collect::<Vec<_>>()),
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

fn target_pixel_value(pixel: &PreparedTargetPixel) -> TargetPixelValue {
    TargetPixelValue {
        fixture_index: pixel.fixture_index() as i32,
        fixture_pixel_index: pixel.fixture_pixel_index() as i32,
        pixel_index: pixel.pixel_index() as i32,
        pixel_count: pixel.pixel_count() as i32,
        pixel_fraction: pixel.pixel_fraction,
    }
}

fn prepared_pixels_from_generated_target(
    fixtures: &[PreparedFixture],
    target: Arc<TargetItemValue>,
) -> Result<Vec<PreparedTargetPixel>, RenderError> {
    target
        .pixels
        .iter()
        .copied()
        .map(|pixel| {
            let fixture_index = usize::try_from(pixel.fixture_index).map_err(|_| {
                RenderError::GeneratorPrepare {
                    message: "generated target fixture index cannot be negative".to_string(),
                }
            })?;
            let fixture_pixel_index = usize::try_from(pixel.fixture_pixel_index).map_err(|_| {
                RenderError::GeneratorPrepare {
                    message: "generated target pixel index cannot be negative".to_string(),
                }
            })?;
            let fixture =
                fixtures
                    .get(fixture_index)
                    .ok_or_else(|| RenderError::GeneratorPrepare {
                        message: "generated target fixture index is out of bounds".to_string(),
                    })?;
            if fixture_pixel_index >= fixture.pixel_count {
                return Err(RenderError::GeneratorPrepare {
                    message: "generated target pixel index is out of bounds".to_string(),
                });
            }
            let pixel_index =
                usize::try_from(pixel.pixel_index).map_err(|_| RenderError::GeneratorPrepare {
                    message: "generated target pixel context index cannot be negative".to_string(),
                })?;
            let pixel_count =
                usize::try_from(pixel.pixel_count).map_err(|_| RenderError::GeneratorPrepare {
                    message: "generated target pixel context count cannot be negative".to_string(),
                })?;
            PreparedTargetPixel::try_new(
                fixture_index,
                fixture_pixel_index,
                pixel_index,
                pixel_count,
                pixel.pixel_fraction,
            )
            .ok_or_else(|| RenderError::GeneratorPrepare {
                message: "generated target pixel exceeds the prepared runtime range".to_string(),
            })
        })
        .collect()
}

pub(crate) fn prepared_pixels_from_generated_target_cached(
    cache: &mut PreparedTargetCache,
    fixtures: &[PreparedFixture],
    target: Arc<TargetItemValue>,
) -> Result<Arc<[PreparedTargetPixel]>, RenderError> {
    let key = arc_key(&target);
    if let Some(entry) = cache.generated_targets.get(&key)
        && Arc::ptr_eq(&entry.source, &target)
    {
        return Ok(Arc::clone(&entry.pixels));
    }
    let pixels = Arc::from(prepared_pixels_from_generated_target(
        fixtures,
        Arc::clone(&target),
    )?);
    cache.generated_targets.insert(
        key,
        GeneratedTargetCacheEntry {
            source: target,
            pixels: Arc::clone(&pixels),
        },
    );
    Ok(pixels)
}

impl PreparedTargetCache {
    pub(crate) fn sample_target(
        &mut self,
        pixels: Arc<[PreparedTargetPixel]>,
    ) -> Result<u32, RenderError> {
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
        let id = u32::try_from(index).map_err(|_| RenderError::BadTarget)?;
        if index == self.sample_targets.len() {
            self.sample_targets.push(pixels);
        }
        Ok(id)
    }
}

#[cfg(test)]
mod representation_tests {
    use super::PreparedTargetPixel;

    #[test]
    fn prepared_target_pixel_stays_32_bit_compact() {
        assert_eq!(std::mem::size_of::<PreparedTargetPixel>(), 16);
    }
}
