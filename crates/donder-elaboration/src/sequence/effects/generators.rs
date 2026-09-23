use donder_language::dsl::{BytecodeProgram, DslBindCache};
use donder_language::effect::EffectDefinitionId;
use donder_language::model::DonderProject;
use donder_language::values::{SampleDuration, SampleTime};
use indexmap::IndexMap;
use std::sync::Arc;

use crate::sequence::targets::{PreparedTargetCache, PreparedTargetPixel};
use crate::{PreparedEffect, PreparedFixture};

#[derive(Clone, Debug)]
pub(crate) struct GeneratorExpansion {
    pub(crate) start_time: SampleTime,
    pub(crate) duration: SampleDuration,
    pub(crate) target: Arc<[PreparedTargetPixel]>,
    pub(crate) depth: usize,
}

pub(crate) struct GeneratorPrepareContext<'a> {
    pub(crate) project: &'a DonderProject,
    pub(crate) fixtures: &'a [PreparedFixture],
    pub(crate) environments: &'a mut Vec<donder_runtime::bindings::PreparedParameterEnvironment>,
    pub(crate) effects: &'a mut Vec<PreparedEffect>,
    pub(crate) generated_child_count: &'a mut usize,
    pub(crate) bind_cache: &'a mut DslBindCache,
    pub(crate) sample_programs: &'a mut IndexMap<EffectDefinitionId, Arc<BytecodeProgram>>,
    pub(crate) target_cache: &'a mut PreparedTargetCache,
}
