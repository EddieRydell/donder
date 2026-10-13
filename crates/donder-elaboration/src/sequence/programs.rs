//! Lowering and interning. Instances of one definition usually lower to the
//! same program with different bound values; playback keeps one copy.
use donder_language::compiler::{Instance, Invocation, ProgramConstants};
use donder_runtime_types::Shared;
use donder_runtime_types::{OperatorInvocation, OperatorProgram, SampleInvocation, SampleProgram};
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};

/// Lowered clip programs kept between preparations. A clip's program depends
/// only on its invocation and program constants, so an unchanged clip reuses
/// its lowering, and so do clips that repeat one invocation. Entries a
/// preparation did not use are dropped when it finishes.
#[derive(Default)]
pub struct PreparationCache {
    samples: HashMap<u64, Vec<CachedSample>>,
    hasher: std::hash::RandomState,
}

struct CachedSample {
    invocation: Invocation,
    constants: ProgramConstants,
    sample: SampleInvocation,
    /// The program's indices of the invocation's live marks parameters.
    live: Box<[u16]>,
    used: bool,
}

impl PreparationCache {
    fn finish(&mut self) {
        self.samples.retain(|_, entries| {
            entries.retain_mut(|entry| std::mem::take(&mut entry.used));
            !entries.is_empty()
        });
    }
}

pub(crate) struct Programs<'cache> {
    samples: Vec<Shared<SampleProgram>>,
    operators: Vec<Shared<OperatorProgram>>,
    cache: &'cache mut PreparationCache,
}

impl Drop for Programs<'_> {
    fn drop(&mut self) {
        self.cache.finish();
    }
}

fn intern<T: PartialEq + Clone>(programs: &mut Vec<Shared<T>>, program: &Shared<T>) -> Shared<T> {
    if let Some(existing) = programs
        .iter()
        .find(|existing| Shared::ptr_eq(existing, program))
        .or_else(|| programs.iter().find(|existing| ***existing == **program))
    {
        return Shared::clone(existing);
    }
    programs.push(Shared::clone(program));
    Shared::clone(program)
}

impl<'cache> Programs<'cache> {
    pub(crate) fn new(cache: &'cache mut PreparationCache) -> Self {
        Self {
            samples: Vec::new(),
            operators: Vec::new(),
            cache,
        }
    }

    /// The program of `invocation` with `constants`, lowered once per
    /// distinct invocation.
    pub(crate) fn sample(
        &mut self,
        invocation: &Invocation,
        constants: ProgramConstants,
    ) -> SampleInvocation {
        self.sample_live(invocation, constants).0
    }

    /// [`Self::sample`], with the program's indices of the invocation's live
    /// marks parameters, in invocation order.
    pub(crate) fn sample_live(
        &mut self,
        invocation: &Invocation,
        constants: ProgramConstants,
    ) -> (SampleInvocation, Box<[u16]>) {
        let mut hasher = self.cache.hasher.build_hasher();
        invocation.hash_same(&mut hasher);
        constants.hash_same(&mut hasher);
        let entries = self.cache.samples.entry(hasher.finish()).or_default();
        if let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry.constants.same(&constants) && entry.invocation.same(invocation))
        {
            entry.used = true;
            // A program cached by an earlier preparation joins this one's bank.
            intern(&mut self.samples, entry.sample.program());
            return (entry.sample.clone(), entry.live.clone());
        }
        let (lowered, live) = invocation.instance(constants).sample_live();
        let sample = SampleInvocation::bind(
            intern(&mut self.samples, lowered.program()),
            lowered.params().iter_values().collect(),
        )
        .and_then(|sample| sample.with_automation(lowered.automation().into()))
        .unwrap_or_else(|_| unreachable!("interning keeps the program's schema"));
        entries.push(CachedSample {
            invocation: invocation.clone(),
            constants,
            sample: sample.clone(),
            live: live.clone(),
            used: true,
        });
        (sample, live)
    }

    pub(crate) fn operator(&mut self, instance: &Instance) -> OperatorInvocation {
        let lowered = instance.operator();
        OperatorInvocation::bind(
            intern(&mut self.operators, lowered.program()),
            lowered.params().iter_values().collect(),
        )
        .and_then(|invocation| invocation.with_automation(lowered.automation().into()))
        .unwrap_or_else(|_| unreachable!("interning keeps the program's schema"))
    }
}
