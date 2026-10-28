//! Lowering and interning. Instances of one definition usually lower to the
//! same program with different bound values; playback keeps one copy.
use donder_language::compiler::{Instance, Invocation, ProgramConstants};
use donder_runtime_types::{BoundProgram, Program, Shared};
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
    program: BoundProgram,
    /// The program's indices of the invocation's live marks parameters.
    live: Box<[u16]>,
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
    programs: Vec<Shared<Program>>,
    cache: &'cache mut PreparationCache,
}

impl Drop for Programs<'_> {
    fn drop(&mut self) {
        self.cache.finish();
    }
}

impl<'cache> Programs<'cache> {
    pub(crate) fn new(cache: &'cache mut PreparationCache) -> Self {
        Self {
            programs: Vec::new(),
            cache,
        }
    }

    /// One copy of each distinct program.
    fn intern(&mut self, program: &Shared<Program>) -> Shared<Program> {
        if let Some(existing) = self
            .programs
            .iter()
            .find(|existing| Shared::ptr_eq(existing, program))
            .or_else(|| {
                self.programs
                    .iter()
                    .find(|existing| ***existing == **program)
            })
        {
            return Shared::clone(existing);
        }
        self.programs.push(Shared::clone(program));
        Shared::clone(program)
    }

    /// `instance` lowered and bound to its values, sharing an equal
    /// program lowered before; with the program's indices of its live
    /// marks parameters, in invocation order.
    pub(crate) fn lower(&mut self, instance: &Instance) -> (BoundProgram, Box<[u16]>) {
        let lowered = instance.lower();
        let program = self.intern(&Shared::new(lowered.program));
        let bound = BoundProgram::bind(program, lowered.values, lowered.automation)
            .unwrap_or_else(|_| unreachable!("lowered values match their program"));
        (bound, lowered.live)
    }

    /// The program of `invocation` with `constants`, lowered once per
    /// distinct invocation.
    pub(crate) fn clip(
        &mut self,
        invocation: &Invocation,
        constants: ProgramConstants,
    ) -> (BoundProgram, Box<[u16]>) {
        let mut hasher = self.cache.hasher.build_hasher();
        invocation.hash_same(&mut hasher);
        constants.hash_same(&mut hasher);
        let key = hasher.finish();
        if let Some(entry) = self.cache.clips.get_mut(&key).and_then(|entries| {
            entries
                .iter_mut()
                .find(|entry| entry.constants.same(&constants) && entry.invocation.same(invocation))
        }) {
            entry.used = true;
            let (program, live) = (entry.program.clone(), entry.live.clone());
            // A program cached by an earlier preparation joins this one's bank.
            self.intern(program.program());
            return (program, live);
        }
        let (program, live) = self.lower(&invocation.instance(constants));
        self.cache.clips.entry(key).or_default().push(CachedClip {
            invocation: invocation.clone(),
            constants,
            program: program.clone(),
            live: live.clone(),
            used: true,
        });
        (program, live)
    }
}
