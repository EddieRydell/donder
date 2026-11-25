//! Lowering clip programs, cached between preparations.
use donder_language::compiler::{Instance, Invocation, LiveWords, ProgramConstants};
use donder_runtime_types::{BoundProgram, Shared};
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};

/// Lowered clip programs kept between preparations. A clip's program depends
/// only on its invocation and program constants, so an unchanged clip reuses
/// its lowering, and so do clips that repeat one invocation. Entries a
/// preparation did not use are dropped when it finishes.
#[derive(Default)]
pub struct PreparationCache {
    clips: HashMap<u64, Vec<CachedClip>>,
    hasher: std::hash::RandomState,
}

struct CachedClip {
    invocation: Invocation,
    constants: ProgramConstants,
    live: Vec<LiveWords>,
    program: BoundProgram,
    used: bool,
}

impl PreparationCache {
    fn finish(&mut self) {
        self.clips.retain(|_, entries| {
            entries.retain_mut(|entry| std::mem::take(&mut entry.used));
            !entries.is_empty()
        });
    }
}

pub(crate) struct Programs<'cache> {
    cache: &'cache mut PreparationCache,
}

impl Drop for Programs<'_> {
    fn drop(&mut self) {
        self.cache.finish();
    }
}

impl<'cache> Programs<'cache> {
    pub(crate) fn new(cache: &'cache mut PreparationCache) -> Self {
        Self { cache }
    }

    /// `instance` lowered and bound to its values.
    pub(crate) fn lower(&mut self, instance: &Instance) -> BoundProgram {
        let lowered = instance.lower();
        BoundProgram::new(Shared::new(lowered.program), lowered.values)
    }

    /// The program of `invocation` with `constants` and `live` words,
    /// lowered once per distinct invocation.
    pub(crate) fn clip(
        &mut self,
        invocation: &Invocation,
        constants: ProgramConstants,
        live: &[LiveWords],
    ) -> BoundProgram {
        let mut hasher = self.cache.hasher.build_hasher();
        invocation.hash_same(&mut hasher);
        constants.hash_same(&mut hasher);
        std::hash::Hash::hash(live, &mut hasher);
        let key = hasher.finish();
        if let Some(entry) = self.cache.clips.get_mut(&key).and_then(|entries| {
            entries.iter_mut().find(|entry| {
                entry.constants.same(&constants)
                    && entry.live == live
                    && entry.invocation.same(invocation)
            })
        }) {
            entry.used = true;
            return entry.program.clone();
        }
        let program = self.lower(&invocation.instance(constants, live));
        self.cache.clips.entry(key).or_default().push(CachedClip {
            invocation: invocation.clone(),
            constants,
            live: live.to_vec(),
            program: program.clone(),
            used: true,
        });
        program
    }
}
