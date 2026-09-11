use dawn_language::effect::{EffectInstId, EffectRef};
use dawn_language::layout::FixtureInstanceId;
use dawn_language::sequence::{MarkCollectionKey, SequenceId};
use dawn_language::setup::SetupId;
use dawn_runtime::dsl::RuntimeError;
use dawn_runtime::signal::EvaluationError;

pub const MAX_GENERATED_EFFECTS: usize = 4_096;

#[derive(Clone, Debug, PartialEq)]
pub enum RenderError {
    InvalidTiming { reason: String },
    MissingSetup { setup_id: SetupId },
    MissingLayout,
    MissingSequence { sequence_id: SequenceId },
    MissingFixture { fixture_id: FixtureInstanceId },
    MissingEffect { effect_id: EffectRef },
    MissingEffectInstance { effect_id: EffectInstId },
    MissingCurve,
    MissingGradient,
    MissingMarkCollection { key: MarkCollectionKey },
    BadTarget,
    BadGraph { message: String },
    EffectVm { message: String },
    GeneratorPrepare { message: String },
    Evaluation(EvaluationError),
}

impl From<RuntimeError> for RenderError {
    fn from(error: RuntimeError) -> Self {
        Self::EffectVm {
            message: error.message,
        }
    }
}

impl From<EvaluationError> for RenderError {
    fn from(error: EvaluationError) -> Self {
        Self::Evaluation(error)
    }
}
