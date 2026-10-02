//! Retained generator parameter environments. Structural expansion belongs to
//! the host; playback executes only numeric bindings and typed VM calculations.
use crate::dsl::{
    BoundParams, RunContext, Type, VmWorkspace,
    bytecode::{BytecodeProgram, ParameterKind, ProgramContext},
};
use crate::signal::{EvaluationError, PreparedAutomation, apply_bound_automation};
use crate::values::{SampleDuration, SampleTime};
use alloc::{boxed::Box, string::ToString, vec::Vec};

#[derive(Clone, Copy, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct ParameterSource {
    pub environment: usize,
    pub parameter: u16,
}

#[derive(Clone, Copy, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedParameterBinding {
    pub parameter: u16,
    pub source: ParameterSource,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedParameterCalculation {
    pub program: BytecodeProgram,
    pub outputs: Box<[Type]>,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedParameterEnvironment {
    #[rkyv(with = crate::wire::Microseconds)]
    pub start_time: SampleTime,
    #[rkyv(with = crate::wire::Microseconds)]
    pub duration: SampleDuration,
    pub params: BoundParams,
    pub types: Box<[Type]>,
    pub bindings: Box<[PreparedParameterBinding]>,
    pub automation: Box<[PreparedAutomation]>,
    pub calculation: Option<PreparedParameterCalculation>,
    pub array_capacity: usize,
    pub array_width: usize,
}

impl PreparedParameterEnvironment {
    pub fn output_types(&self) -> &[Type] {
        self.calculation
            .as_ref()
            .map_or(&self.types, |calculation| &calculation.outputs)
    }

    /// Conservative result-arena bound shared by host preparation and wire admission.
    /// Result copying preserves shared array nodes, so source capacities can be
    /// added; repeated references do not multiply their subtrees in the result.
    pub fn required_array_storage<'a>(
        parents: impl IntoIterator<Item = &'a Self>,
        calculation: Option<&PreparedParameterCalculation>,
    ) -> (usize, usize) {
        let mut capacity = 0usize;
        let mut width = 0usize;
        for parent in parents {
            // Saturation represents an arena larger than addressable memory.
            // It cannot be allocated; it is not a smaller, permissive bound.
            capacity = capacity.saturating_add(parent.array_capacity);
            width = width.max(parent.array_width);
        }
        if let Some(calculation) = calculation
            && calculation
                .outputs
                .iter()
                .any(|ty| matches!(ty, Type::Array(_)))
        {
            capacity = capacity.saturating_add(calculation.program.array_capacity as usize);
            width = width.max(calculation.program.array_width as usize);
        }
        (capacity, width)
    }

    pub fn validate_all(environments: &[Self]) -> Result<(), EvaluationError> {
        for (index, environment) in environments.iter().enumerate() {
            if environment.duration.as_ticks() == 0
                || environment.params.len() != environment.types.len()
                || !environment.params.is_frozen()
                || !environment.params.has_type_layout(&environment.types)
                || !environment
                    .params
                    .has_valid_automation(&environment.automation)
                || (environment.array_capacity != 0 && environment.array_width == 0)
            {
                return Err(invalid("invalid prepared parameter environment"));
            }
            for (slot, ty) in environment.types.iter().enumerate() {
                if !environment
                    .bindings
                    .iter()
                    .any(|binding| usize::from(binding.parameter) == slot)
                    && !ty.accepts_value(&environment.params.value(slot)?)
                {
                    return Err(invalid(
                        "uninitialized or invalid parameter environment slot",
                    ));
                }
            }
            for (binding_index, binding) in environment.bindings.iter().enumerate() {
                if binding.source.environment >= index {
                    return Err(invalid(
                        "parameter environments must reference earlier environments",
                    ));
                }
                let source = environments[binding.source.environment]
                    .output_types()
                    .get(usize::from(binding.source.parameter));
                let destination = environment.types.get(usize::from(binding.parameter));
                if source.is_none()
                    || destination.is_none()
                    || !source
                        .zip(destination)
                        .is_some_and(|(source, destination)| destination.accepts(source))
                    || environment.bindings[..binding_index]
                        .iter()
                        .any(|previous| previous.parameter == binding.parameter)
                {
                    return Err(invalid("invalid typed parameter binding"));
                }
            }
            for automation in &environment.automation {
                if usize::from(automation.param_index) >= environment.types.len()
                    || !automation
                        .mapping
                        .accepts_type(&environment.types[usize::from(automation.param_index)])
                    || environment
                        .bindings
                        .iter()
                        .any(|binding| binding.parameter == automation.param_index)
                {
                    return Err(invalid("invalid parameter environment automation"));
                }
            }
            if let Some(calculation) = &environment.calculation {
                let program = &calculation.program;
                if !program.has_valid_structure()
                    || !program.has_valid_context(ProgramContext::Calculation)
                    || !program.has_valid_calculation_outputs(&calculation.outputs)
                    || !program.has_valid_parameter_reads(|index| {
                        environment.types.get(index).map(ParameterKind::for_type)
                    })
                    || !program.has_valid_reference_parameter_reads(|index, expected| {
                        environment
                            .types
                            .get(index)
                            .is_some_and(|actual| expected.accepts(actual))
                    })
                {
                    return Err(invalid(
                        "parameter calculations cannot read pixel or signal context",
                    ));
                }
            }
            if Self::required_array_storage(
                environment
                    .bindings
                    .iter()
                    .map(|binding| &environments[binding.source.environment]),
                environment.calculation.as_ref(),
            ) != (environment.array_capacity, environment.array_width)
            {
                return Err(invalid("invalid parameter array storage"));
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct EnvironmentWorkspace {
    params: BoundParams,
    outputs: BoundParams,
    vm: VmWorkspace,
    ready: bool,
    needed: bool,
}

impl EnvironmentWorkspace {
    fn output(&self, environment: &PreparedParameterEnvironment) -> &BoundParams {
        if environment.calculation.is_some() {
            &self.outputs
        } else {
            &self.params
        }
    }
}

#[derive(Debug, Default)]
struct ParameterTimeWorkspace {
    environments: Vec<EnvironmentWorkspace>,
    sample_time: Option<SampleTime>,
}

#[derive(Debug, Default)]
pub struct ParameterWorkspace {
    times: Vec<ParameterTimeWorkspace>,
    recency: Vec<u64>,
    request: u64,
}

impl ParameterWorkspace {
    pub fn new(environments: &[PreparedParameterEnvironment], time_slots: usize) -> Self {
        Self {
            times: (0..time_slots)
                .map(|_| ParameterTimeWorkspace::new(environments))
                .collect(),
            recency: alloc::vec![0; time_slots],
            request: 0,
        }
    }

    pub(crate) fn storage_estimate(
        environments: &[PreparedParameterEnvironment],
        time_slots: usize,
    ) -> Option<usize> {
        ParameterTimeWorkspace::storage_estimate(environments)?
            .checked_add(size_of::<ParameterTimeWorkspace>() + size_of::<u64>())?
            .checked_mul(time_slots)
    }

    pub fn resolve(
        &mut self,
        environments: &[PreparedParameterEnvironment],
        index: usize,
        time: SampleTime,
    ) -> Result<&BoundParams, EvaluationError> {
        let slot = self
            .times
            .iter()
            .position(|state| state.sample_time == Some(time))
            .or_else(|| {
                self.recency
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, used)| *used)
                    .map(|(index, _)| index)
            })
            .ok_or(EvaluationError::InvalidWorkspace)?;
        self.request = self.request.wrapping_add(1);
        self.recency[slot] = self.request;
        self.times[slot].resolve(environments, index, time)
    }
}

fn invalid(message: &str) -> EvaluationError {
    EvaluationError::InvalidGraph {
        message: message.to_string(),
    }
}

impl ParameterTimeWorkspace {
    pub(crate) fn storage_estimate(environments: &[PreparedParameterEnvironment]) -> Option<usize> {
        let mut bytes = environments
            .len()
            .checked_mul(size_of::<EnvironmentWorkspace>())?;
        for environment in environments {
            bytes = bytes
                .checked_add(
                    environment
                        .params
                        .automation_storage_estimate(&environment.automation)?,
                )?
                .checked_add(BoundParams::result_storage_estimate(
                    0,
                    environment.array_capacity,
                    environment.array_width,
                )?)?;
            if let Some(calculation) = &environment.calculation {
                let program = &calculation.program;
                let layout = program.layout;
                bytes = bytes
                    .checked_add(BoundParams::result_storage_estimate(
                        calculation.outputs.len(),
                        environment.array_capacity,
                        environment.array_width,
                    )?)?
                    .checked_add(
                        VmWorkspace::storage_estimate(
                            [
                                layout.ints,
                                layout.floats,
                                layout.bools,
                                layout.colors,
                                layout.arrays,
                                layout.marks,
                                layout.curves,
                                layout.gradients,
                                layout.targets,
                                layout.target_lists,
                                layout.target_items,
                                layout.enums,
                            ]
                            .map(|count| count as usize),
                            program.array_capacity as usize,
                            program.array_width as usize,
                            program.loop_count as usize,
                        )?
                        .checked_sub(size_of::<VmWorkspace>())?,
                    )?;
            }
        }
        Some(bytes)
    }

    fn new(environments: &[PreparedParameterEnvironment]) -> Self {
        Self {
            environments: environments
                .iter()
                .map(|environment| {
                    let mut params = environment.params.clone_for_automation();
                    params
                        .reserve_result_arrays(environment.array_capacity, environment.array_width);
                    for binding in &environment.automation {
                        params.reserve_automation(
                            usize::from(binding.param_index),
                            &binding.curve,
                            &binding.mapping,
                        );
                    }
                    let (outputs, vm) = environment.calculation.as_ref().map_or_else(
                        || (BoundParams::default(), VmWorkspace::default()),
                        |calculation| {
                            (
                                BoundParams::result_workspace(
                                    &calculation.outputs,
                                    environment.array_capacity,
                                    environment.array_width,
                                ),
                                VmWorkspace::for_program(&calculation.program),
                            )
                        },
                    );
                    EnvironmentWorkspace {
                        params,
                        outputs,
                        vm,
                        ready: false,
                        needed: false,
                    }
                })
                .collect(),
            sample_time: None,
        }
    }

    fn resolve(
        &mut self,
        environments: &[PreparedParameterEnvironment],
        index: usize,
        time: SampleTime,
    ) -> Result<&BoundParams, EvaluationError> {
        if self.environments.len() != environments.len() || index >= environments.len() {
            return Err(EvaluationError::InvalidWorkspace);
        }
        if self.sample_time != Some(time) {
            // All descendant references must be gone before any root curve is
            // updated. Otherwise its copy-on-write update would allocate.
            for (state, environment) in self.environments.iter_mut().zip(environments).rev() {
                state.outputs.clear_results();
                for binding in &environment.bindings {
                    state.params.clear_parameter(usize::from(binding.parameter));
                }
                state.ready = false;
            }
            self.sample_time = Some(time);
        }
        if self.environments[index].ready {
            return Ok(self.environments[index].output(&environments[index]));
        }
        for state in &mut self.environments {
            state.needed = false;
        }
        self.environments[index].needed = true;
        for dependency in (0..=index).rev() {
            if !self.environments[dependency].needed || self.environments[dependency].ready {
                continue;
            }
            for binding in &environments[dependency].bindings {
                self.environments[binding.source.environment].needed = true;
            }
        }
        for dependency in 0..=index {
            if self.environments[dependency].needed {
                self.evaluate(environments, dependency, time)?;
            }
        }
        Ok(self.environments[index].output(&environments[index]))
    }

    fn evaluate(
        &mut self,
        environments: &[PreparedParameterEnvironment],
        index: usize,
        time: SampleTime,
    ) -> Result<(), EvaluationError> {
        if self.environments[index].ready {
            return Ok(());
        }
        let environment = &environments[index];
        let (ancestors, current) = self.environments.split_at_mut(index);
        let state = &mut current[0];
        for binding in &environment.bindings {
            let source = binding.source.environment;
            state.params.copy_parameter(
                usize::from(binding.parameter),
                ancestors[source].output(&environments[source]),
                usize::from(binding.source.parameter),
                &environment.types[usize::from(binding.parameter)],
            )?;
        }
        apply_bound_automation(&mut state.params, &environment.automation, time)?;
        if let Some(calculation) = &environment.calculation {
            let elapsed = time
                .checked_duration_since(environment.start_time)
                .unwrap_or(SampleDuration::from_ticks(0));
            let context = RunContext {
                progress: (elapsed.as_ticks() as f32 / environment.duration.as_ticks() as f32)
                    .clamp(0.0, 1.0),
                time: elapsed,
                duration: environment.duration,
                pixel_index: 0,
                pixel_count: 0,
                pixel_fraction: 0.0,
            };
            calculation.program.evaluate_bindings(
                &state.params,
                &context,
                &mut state.vm,
                &mut state.outputs,
                &calculation.outputs,
            )?;
        }
        state.ready = true;
        Ok(())
    }
}
