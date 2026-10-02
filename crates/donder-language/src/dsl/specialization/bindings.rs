//! Lexical values and playback references have separate storage. A compiled
//! structural calculation reads only slots proven fixed by staging analysis.
use super::{BindingSlot, FixedBindingSlot, GeneratorBinding, Value};

#[derive(Clone, Copy)]
enum Source {
    Constant,
    Parameter(u16),
    Calculation { index: usize, output: u16 },
}

pub(super) struct Bindings {
    constants: Vec<Value>,
    sources: Vec<Source>,
}

impl Bindings {
    pub(super) fn new(count: usize) -> Self {
        Self {
            // Compiled declarations initialize locals before any read. Void is
            // unused storage, not a default substituted for a missing input.
            constants: vec![Value::Void; count],
            sources: vec![Source::Constant; count],
        }
    }

    pub(super) fn assign(&mut self, slot: BindingSlot, binding: GeneratorBinding) {
        let (value, source) = match binding {
            GeneratorBinding::Constant(value) => (value, Source::Constant),
            GeneratorBinding::Parameter(index) => (Value::Void, Source::Parameter(index)),
            GeneratorBinding::Calculation { index, output } => {
                (Value::Void, Source::Calculation { index, output })
            }
        };
        self.constants[slot.0] = value;
        self.sources[slot.0] = source;
    }

    pub(super) fn read(&self, slot: BindingSlot) -> GeneratorBinding {
        match self.sources[slot.0] {
            Source::Constant => GeneratorBinding::Constant(self.constants[slot.0].clone()),
            Source::Parameter(index) => GeneratorBinding::Parameter(index),
            Source::Calculation { index, output } => {
                GeneratorBinding::Calculation { index, output }
            }
        }
    }

    pub(super) fn fixed(&self, slot: FixedBindingSlot) -> Value {
        self.constants[slot.0.0].clone()
    }
}
