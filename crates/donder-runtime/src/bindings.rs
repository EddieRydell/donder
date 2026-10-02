//! Retained generator parameter environments. Structural expansion belongs to
//! the host; playback executes only numeric bindings and typed VM calculations.
use crate::dsl::RuntimeError;
use crate::dsl::{
    AutomationPlan, BoundParams, CalculationProgram, ParameterTransfer, RunContext, Type,
    VmWorkspace,
    bytecode::{BytecodeProgram, ParameterKind, ProgramContext},
};
use crate::signal::PreparedAutomation;
use crate::values::{SampleDuration, SampleTime};
use alloc::{boxed::Box, string::ToString, vec::Vec};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct ParameterSource {
    pub(crate) environment: usize,
    pub(crate) parameter: u16,
}

#[derive(Clone, Copy, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedParameterBinding {
    pub(crate) parameter: u16,
    pub(crate) source: ParameterSource,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedParameterCalculation {
    pub(crate) program: BytecodeProgram,
    pub(crate) outputs: Box<[Type]>,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedParameterEnvironment<
    C = PreparedParameterCalculation,
    B = PreparedParameterBinding,
    A = Box<[PreparedAutomation]>,
> {
    #[rkyv(with = crate::wire::Microseconds)]
    pub(crate) start_time: SampleTime,
    #[rkyv(with = crate::wire::Microseconds)]
    pub(crate) duration: SampleDuration,
    pub(crate) params: BoundParams,
    pub(crate) types: Box<[Type]>,
    pub(crate) bindings: Box<[B]>,
    pub(crate) automation: A,
    pub(crate) calculation: Option<C>,
    pub(crate) array_capacity: usize,
    pub(crate) array_width: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedParameterBinding {
    binding: PreparedParameterBinding,
    transfer: ParameterTransfer,
}

impl ResolvedParameterBinding {
    pub(crate) fn source_environment(&self) -> usize {
        self.binding.source.environment
    }

    /// Compaction preserves the source's bank layout and only changes its owner slot.
    pub(crate) fn remap_environment(&mut self, environment: usize) {
        self.binding.source.environment = environment;
    }

    pub(crate) fn from_linked(
        binding: PreparedParameterBinding,
        transfer: ParameterTransfer,
    ) -> Self {
        Self { binding, transfer }
    }
}

pub(crate) type ExecutableEnvironment =
    PreparedParameterEnvironment<CalculationProgram, ResolvedParameterBinding, AutomationPlan>;

impl ExecutableEnvironment {
    /// A plain forwarding edge, not an evaluated value or an automated slot.
    /// Lowering may bypass it when source and destination types are identical;
    /// conversions must retain their declared intermediate layout.
    pub(crate) fn forwarded_source(&self, parameter: u16) -> Option<ParameterSource> {
        if self.calculation.is_some() || !self.automation.is_empty() {
            return None;
        }
        self.bindings
            .iter()
            .find(|binding| binding.binding.parameter == parameter)
            .map(|binding| binding.binding.source)
    }

    pub(crate) fn output_types(&self) -> &[Type] {
        self.calculation
            .as_ref()
            .map_or(&self.types, CalculationProgram::output_types)
    }

    pub(crate) fn to_raw(&self) -> PreparedParameterEnvironment {
        PreparedParameterEnvironment {
            start_time: self.start_time,
            duration: self.duration,
            params: self.params.clone(),
            types: self.types.clone(),
            bindings: self
                .bindings
                .iter()
                .map(|binding| binding.binding)
                .collect(),
            automation: self.automation.to_raw(),
            calculation: self.calculation.as_ref().map(|calculation| {
                let (program, _, outputs) = calculation.clone().into_parts();
                PreparedParameterCalculation { program, outputs }
            }),
            array_capacity: self.array_capacity,
            array_width: self.array_width,
        }
    }
}

/// Raw archives and externally assembled graphs are checked before the graph
/// publishes these immutable execution plans.
pub(crate) fn admit_environments(
    environments: Vec<PreparedParameterEnvironment>,
) -> Result<Box<[ExecutableEnvironment]>, RuntimeError> {
    PreparedParameterEnvironment::validate_all(&environments)?;
    let outputs: Vec<_> = environments
        .iter()
        .map(|environment| BoundParams::result_workspace(environment.output_types(), 0, 0))
        .collect();
    environments
        .into_iter()
        .map(|environment| {
            let automation = AutomationPlan::admit(&environment.params, &environment.automation)
                .ok_or_else(|| invalid("invalid parameter automation"))?;
            let bindings = environment
                .bindings
                .iter()
                .map(|binding| {
                    Ok(ResolvedParameterBinding {
                        binding: *binding,
                        transfer: ParameterTransfer::admit(
                            &outputs[binding.source.environment],
                            &environment.params,
                            usize::from(binding.source.parameter),
                            usize::from(binding.parameter),
                        )
                        .ok_or_else(|| invalid("invalid parameter transfer"))?,
                    })
                })
                .collect::<Result<_, RuntimeError>>()?;
            let calculation = environment
                .calculation
                .map(|calculation| {
                    CalculationProgram::new(
                        calculation.program,
                        environment.types.clone(),
                        calculation.outputs,
                    )
                    .ok_or_else(|| invalid("invalid retained calculation"))
                })
                .transpose()?;
            Ok(PreparedParameterEnvironment {
                start_time: environment.start_time,
                duration: environment.duration,
                params: environment.params,
                types: environment.types,
                bindings,
                automation,
                calculation,
                array_capacity: environment.array_capacity,
                array_width: environment.array_width,
            })
        })
        .collect()
}

impl PreparedParameterEnvironment {
    pub(crate) fn output_types(&self) -> &[Type] {
        self.calculation
            .as_ref()
            .map_or(&self.types, |calculation| &calculation.outputs)
    }

    /// Conservative result-arena bound shared by host preparation and wire admission.
    /// Result copying preserves shared array nodes, so source capacities can be
    /// added; repeated references do not multiply their subtrees in the result.
    pub(crate) fn required_array_storage<'a>(
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

    pub(crate) fn validate_all(environments: &[Self]) -> Result<(), RuntimeError> {
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
    automation: AutomationPlan,
    ready: bool,
    needed: bool,
}

impl EnvironmentWorkspace {
    fn output(&self, environment: &ExecutableEnvironment) -> &BoundParams {
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
pub(crate) struct ParameterWorkspace {
    times: Vec<ParameterTimeWorkspace>,
    recency: Vec<u64>,
    request: u64,
}

impl ParameterWorkspace {
    pub(crate) fn new(environments: &[ExecutableEnvironment], time_slots: usize) -> Self {
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

    pub(crate) fn resolve(
        &mut self,
        environments: &[ExecutableEnvironment],
        index: usize,
        time: SampleTime,
    ) -> &BoundParams {
        let slot = self
            .times
            .iter()
            .position(|state| state.sample_time == Some(time))
            .unwrap_or_else(|| {
                (1..self.recency.len()).fold(0, |oldest, index| {
                    if self.recency[index] < self.recency[oldest] {
                        index
                    } else {
                        oldest
                    }
                })
            });
        self.request = self.request.wrapping_add(1);
        self.recency[slot] = self.request;
        self.times[slot].resolve(environments, index, time)
    }
}

fn invalid(message: &str) -> RuntimeError {
    RuntimeError {
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
                .checked_add(AutomationPlan::storage_estimate(&environment.automation)?)?
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

    fn new(environments: &[ExecutableEnvironment]) -> Self {
        Self {
            environments: environments
                .iter()
                .map(|environment| {
                    let mut params = environment.params.clone();
                    params
                        .reserve_result_arrays(environment.array_capacity, environment.array_width);
                    let (outputs, vm) = environment.calculation.as_ref().map_or_else(
                        || (BoundParams::default(), VmWorkspace::default()),
                        |calculation| {
                            (
                                BoundParams::result_workspace(
                                    calculation.output_types(),
                                    environment.array_capacity,
                                    environment.array_width,
                                ),
                                VmWorkspace::for_program(calculation.bytecode()),
                            )
                        },
                    );
                    EnvironmentWorkspace {
                        params,
                        outputs,
                        vm,
                        automation: environment.automation.clone(),
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
        environments: &[ExecutableEnvironment],
        index: usize,
        time: SampleTime,
    ) -> &BoundParams {
        if self.sample_time != Some(time) {
            // All descendant references must be gone before any root curve is
            // updated. Otherwise its copy-on-write update would allocate.
            for (state, environment) in self.environments.iter_mut().zip(environments).rev() {
                state.outputs.clear_results();
                for binding in &environment.bindings {
                    state
                        .params
                        .clear_parameter(usize::from(binding.binding.parameter));
                }
                state.ready = false;
            }
            self.sample_time = Some(time);
        }
        if self.environments[index].ready {
            return self.environments[index].output(&environments[index]);
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
                self.environments[binding.binding.source.environment].needed = true;
            }
        }
        for dependency in 0..=index {
            if self.environments[dependency].needed {
                self.evaluate(environments, dependency, time);
            }
        }
        self.environments[index].output(&environments[index])
    }

    fn evaluate(&mut self, environments: &[ExecutableEnvironment], index: usize, time: SampleTime) {
        if self.environments[index].ready {
            return;
        }
        let environment = &environments[index];
        let (ancestors, current) = self.environments.split_at_mut(index);
        let state = &mut current[0];
        for binding in &environment.bindings {
            let source = binding.binding.source.environment;
            binding.transfer.apply(
                ancestors[source].output(&environments[source]),
                &mut state.params,
            );
        }
        state.automation.apply(&mut state.params, time);
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
            calculation.evaluate_retained(
                &state.params,
                &context,
                &mut state.vm,
                &mut state.outputs,
            );
        }
        state.ready = true;
    }
}
