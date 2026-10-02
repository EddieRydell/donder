use crate::automation::AutomationMapping;
use crate::dsl::AutomationPlan;
use crate::dsl::bytecode::BytecodeProgram;
use crate::dsl::{BoundParams, OperatorProgram, SampleProgram, VmWorkspace};
use crate::sequence::programs::ExecutableGraph;
use crate::values::{Color, Curve, SampleDuration, SampleTime};
use alloc::boxed::Box;
#[cfg(not(feature = "atomic"))]
use alloc::rc::Rc as Arc;
#[cfg(feature = "atomic")]
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

/// Raw construction/archive data. This representation is not executable by itself:
/// Archive admission checks it before publishing immutable playback state.
/// The private executable graph substitutes admitted programs and binding plans.
#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedSignalGraph<
    P = Box<[BytecodeProgram]>,
    E = crate::bindings::PreparedParameterEnvironment,
    A = Box<[PreparedAutomation]>,
> {
    pub parameter_environments: Box<[E]>,
    pub frame_rate: u32,
    pub frame_count: u32,
    #[rkyv(with = crate::wire::Microseconds)]
    pub duration: SampleDuration,
    pub fixtures: Box<[PreparedFixture]>,
    pub fixture_pixel_offsets: Box<[usize]>,
    pub pixel_count: usize,
    pub effects: Box<[PreparedEffect<A>]>,
    /// Authored clip groups, including children expanded from generators.
    pub clips: Box<[PreparedClip]>,
    pub programs: P,
    pub targets: Box<[PreparedTarget]>,
    pub target_pixels: Box<[PreparedPixel]>,
    pub spatial_contexts: Box<[crate::dsl::SpatialContext]>,
    pub effects_by_layer: Box<[Box<[usize]>]>,
    pub layers: Box<[PreparedLayer]>,
    pub plan: SignalPlan<A>,
}

/// A borrowed graph whose executable programs and environments were admitted together.
#[derive(Clone, Copy)]
pub(crate) struct SignalGraph<'a> {
    pub(crate) data: &'a ExecutableGraph,
}

impl core::ops::Deref for SignalGraph<'_> {
    type Target = ExecutableGraph;
    fn deref(&self) -> &Self::Target {
        self.data
    }
}

/// Numeric source identity and sampling domain for an authored timeline clip.
/// Playback effects can be reordered or filtered without losing clip ownership.
#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedClip {
    pub id: u32,
    #[rkyv(with = crate::wire::Microseconds)]
    pub start_time: SampleTime,
    #[rkyv(with = crate::wire::Microseconds)]
    pub duration: SampleDuration,
    pub target: usize,
    pub effects: Box<[usize]>,
}

#[derive(Clone, Copy, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedFixture {
    pub id: u32,
    pub pixel_count: usize,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedEffect<A = Box<[PreparedAutomation]>> {
    #[rkyv(with = crate::wire::Microseconds)]
    pub start_time: SampleTime,
    #[rkyv(with = crate::wire::Microseconds)]
    pub duration: SampleDuration,
    pub target: usize,
    pub implementation: PreparedEffectImplementation,
    pub automation: Option<Box<PreparedEffectAutomation<A>>>,
}

impl<A> PreparedEffect<A> {
    pub(crate) fn is_active(&self, sample_time: SampleTime) -> bool {
        sample_time >= self.start_time
            && self
                .start_time
                .checked_add_duration(self.duration)
                .is_some_and(|end| sample_time < end)
    }

    pub(crate) fn local_time(&self, sample_time: SampleTime) -> SampleDuration {
        sample_time
            .checked_duration_since(self.start_time)
            .unwrap_or(SampleDuration::from_ticks(0))
    }

    pub(crate) fn progress(&self, sample_time: SampleTime) -> f32 {
        let elapsed = sample_time
            .checked_duration_since(self.start_time)
            .map_or(0, |duration| duration.as_ticks());
        (elapsed as f32 / self.duration.as_ticks() as f32).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum PreparedEffectImplementation {
    Bound {
        environment: usize,
        program: usize,
    },
    Dsl {
        program: usize,
        bound_params: BoundParams,
    },
}

impl PreparedEffectImplementation {
    pub(crate) fn dsl_program(&self) -> usize {
        match self {
            Self::Dsl { program, .. } | Self::Bound { program, .. } => *program,
        }
    }
}

#[derive(Clone, Copy, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedLayer {
    pub enabled: bool,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedEffectAutomation<A = Box<[PreparedAutomation]>> {
    /// Dense index in automated-effect order, assigned by elaboration.
    pub workspace_slot: usize,
    pub bindings: A,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedAutomation {
    #[rkyv(with = crate::wire::Microseconds)]
    pub start: SampleTime,
    #[rkyv(with = crate::wire::Microseconds)]
    pub duration: SampleDuration,
    pub curve: Arc<Curve>,
    pub mapping: AutomationMapping,
    pub param_index: u16,
}

impl PreparedAutomation {
    pub fn position(&self, sample_time: SampleTime) -> f32 {
        let elapsed = sample_time
            .checked_duration_since(self.start)
            .map_or(0, |duration| duration.as_ticks());
        if self.duration.as_ticks() == 0 {
            0.0
        } else {
            (elapsed as f32 / self.duration.as_ticks() as f32).clamp(0.0, 1.0)
        }
    }
}

/// Graph connections and the buffer/VM schedule assigned during elaboration.
#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct SignalPlan<A = Box<[PreparedAutomation]>> {
    pub output_index: usize,
    pub target: usize,
    pub nodes: Box<[PreparedSignalNode<A>]>,
    pub vm_workspace_count: usize,
    pub frame_nodes: Box<[usize]>,
    pub frame_slots: Box<[usize]>,
    pub frame_buffer_count: usize,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedSignalNode<A = Box<[PreparedAutomation]>> {
    pub kind: PreparedSignalKind<A>,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum PreparedSignalKind<A = Box<[PreparedAutomation]>> {
    Layer {
        layer_index: usize,
    },
    Operator {
        operator: PreparedOperatorNode,
        inputs: Box<[usize]>,
        automation: A,
        vm_slot: usize,
    },
    Output {
        inputs: Box<[usize]>,
    },
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum PreparedOperator {
    Dsl(usize),
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedOperatorNode {
    /// Dense index among automated graph nodes; unused without bindings.
    pub automation_slot: usize,
    pub implementation: PreparedOperator,
    pub params: BoundParams,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedTarget {
    pub pixels: core::ops::Range<usize>,
    pub sections: crate::sections::PreparedSections,
    /// Zero disables sample reuse; otherwise this is the required cache width.
    pub sample_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedPixel {
    pub fixture_index: usize,
    pub fixture_pixel_index: u32,
    pub pixel_index: usize,
    pub pixel_count: usize,
    pub pixel_fraction: f32,
}

impl PreparedPixel {
    pub fn try_new(
        fixture_index: usize,
        fixture_pixel_index: usize,
        pixel_index: usize,
        pixel_count: usize,
        pixel_fraction: f32,
    ) -> Option<Self> {
        Some(Self {
            fixture_index,
            fixture_pixel_index: u32::try_from(fixture_pixel_index).ok()?,
            pixel_index,
            pixel_count,
            pixel_fraction,
        })
    }

    pub fn fixture_index(&self) -> usize {
        self.fixture_index
    }

    pub fn fixture_pixel_index(&self) -> usize {
        self.fixture_pixel_index as usize
    }

    pub fn pixel_index(&self) -> usize {
        self.pixel_index
    }

    pub fn pixel_count(&self) -> usize {
        self.pixel_count
    }
}

#[derive(Debug)]
pub(crate) struct EvaluationWorkspace {
    pub(crate) parameters: crate::bindings::ParameterWorkspace,
    pub(crate) effect_vm: VmWorkspace,
    pub(crate) effect_vm_sample: Option<(CachedVmSample, SampleDuration, Color)>,
    pub(crate) operator_vm: Vec<(VmWorkspace, Option<CachedVmSample>)>,
    pub(crate) operator_frames: Vec<Vec<CachedSignalFrame>>,
    pub(crate) signal_cache: Box<[Option<CachedSignal>]>,
    pub(crate) signal_buffers: Box<[Color]>,
    pub(crate) frame_scratch: Vec<Box<[Color]>>,
    pub(crate) frame_scratch_used: usize,
    pub(crate) effect_samples: Vec<CachedEffectSample>,
    pub(crate) effect_automation: Vec<EffectAutomationWorkspace>,
    pub(crate) operator_automation: Vec<EffectAutomationWorkspace>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CachedVmSample {
    pub(crate) index: usize,
    pub(crate) time: SampleTime,
    pub(crate) progress: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CachedSignal {
    pub(crate) sample_time: SampleTime,
    pub(crate) flat_pixel_index: usize,
    pub(crate) color: Color,
}

#[derive(Debug)]
pub(crate) struct CachedSignalFrame {
    pub(crate) key: Option<(usize, SampleTime)>,
    pub(crate) colors: Box<[Color]>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CachedEffectSample {
    pub(crate) pixel_count: usize,
    pub(crate) color: Color,
}

#[derive(Debug, Default)]
pub(crate) struct EffectAutomationWorkspace {
    pub(crate) plan: AutomationPlan,
    pub(crate) params: BoundParams,
    pub(crate) sample_time: Option<SampleTime>,
}

impl EffectAutomationWorkspace {
    pub(crate) fn params_at(&mut self, time: SampleTime) -> &BoundParams {
        if self.sample_time != Some(time) {
            self.plan.apply(&mut self.params, time);
            self.sample_time = Some(time);
        }
        &self.params
    }
}

impl PreparedSignalGraph {
    /// Keep independent temporal query sites resident while visiting pixels.
    /// Looping sites can replace their own old requested times in this bounded
    /// cache; identities always include the exact requested SampleTime.
    pub(crate) fn parameter_time_slots(&self) -> Result<usize, crate::wire::LoadError> {
        if self.parameter_environments.is_empty() {
            return Ok(0);
        }
        let mut slots = 1usize;
        for node in &self.plan.nodes {
            if let PreparedSignalKind::Operator { operator, .. } = &node.kind {
                slots = slots.saturating_add(match operator.implementation {
                    PreparedOperator::Dsl(program) => self
                        .programs
                        .get(program)
                        .ok_or(crate::wire::LoadError::InvalidSequence)?
                        .instructions
                        .iter()
                        .filter(|instruction| {
                            matches!(
                                instruction,
                                crate::dsl::bytecode::Instruction::SignalSample { .. }
                            )
                        })
                        .count(),
                });
            }
        }
        Ok(slots)
    }
    /// Maximum number of temporary frames held by nested whole-frame sampling.
    /// DSL frame caches own their storage separately, but may sample operators.
    pub(crate) fn frame_scratch_count(&self) -> usize {
        self.frame_scratch_count_with(|program| self.programs[program].frame_cache_count())
    }
}

impl
    PreparedSignalGraph<
        crate::sequence::programs::AdmittedPrograms,
        crate::bindings::ExecutableEnvironment,
        AutomationPlan,
    >
{
    pub(crate) fn sample_program(&self, index: usize) -> &SampleProgram {
        self.programs.sample(index)
    }

    pub(crate) fn operator_program(&self, index: usize) -> &OperatorProgram {
        self.programs.operator(index)
    }

    pub(crate) fn parameter_time_slots(&self) -> usize {
        if self.parameter_environments.is_empty() {
            return 0;
        }
        self.plan.nodes.iter().fold(1usize, |slots, node| {
            let PreparedSignalKind::Operator { operator, .. } = &node.kind else {
                return slots;
            };
            let PreparedOperator::Dsl(program) = operator.implementation;
            slots.saturating_add(
                self.operator_program(program)
                    .bytecode()
                    .instructions
                    .iter()
                    .filter(|instruction| {
                        matches!(
                            instruction,
                            crate::dsl::bytecode::Instruction::SignalSample { .. }
                        )
                    })
                    .count(),
            )
        })
    }

    pub(crate) fn frame_scratch_count(&self) -> usize {
        self.frame_scratch_count_with(|program| {
            operator_frame_cache_count(self.operator_program(program))
        })
    }

    /// Preallocates frame buffers, VM registers, calculated-array slots,
    /// and automation storage after admission.
    pub(crate) fn create_workspace(&self) -> EvaluationWorkspace {
        let mut operator_frame_counts = vec![0usize; self.plan.vm_workspace_count];
        for node in &self.plan.nodes {
            let PreparedSignalKind::Operator {
                operator:
                    PreparedOperatorNode {
                        implementation: PreparedOperator::Dsl(program),
                        ..
                    },
                vm_slot,
                ..
            } = &node.kind
            else {
                continue;
            };
            let count = operator_frame_cache_count(self.operator_program(*program));
            operator_frame_counts[*vm_slot] = operator_frame_counts[*vm_slot].max(count);
        }
        let mut workspace = EvaluationWorkspace {
            parameters: crate::bindings::ParameterWorkspace::new(
                &self.parameter_environments,
                self.parameter_time_slots(),
            ),
            effect_vm: VmWorkspace::default(),
            frame_scratch: (0..self.frame_scratch_count())
                .map(|_| vec![Color::BLACK; self.pixel_count].into_boxed_slice())
                .collect(),
            frame_scratch_used: 0,
            effect_vm_sample: None,
            operator_vm: (0..self.plan.vm_workspace_count)
                .map(|_| (VmWorkspace::default(), None))
                .collect(),
            operator_frames: operator_frame_counts
                .into_iter()
                .map(|count| {
                    (0..count)
                        .map(|_| CachedSignalFrame {
                            key: None,
                            colors: vec![
                                Color {
                                    red: 0,
                                    green: 0,
                                    blue: 0,
                                };
                                self.pixel_count
                            ]
                            .into_boxed_slice(),
                        })
                        .collect()
                })
                .collect(),
            signal_cache: vec![
                None;
                if self.plan.frame_nodes.iter().any(|&index| {
                    matches!(
                        &self.plan.nodes[index].kind,
                        PreparedSignalKind::Operator { .. }
                    )
                }) {
                    self.plan.nodes.len()
                } else {
                    0
                }
            ]
            .into_boxed_slice(),
            signal_buffers: vec![
                Color {
                    red: 0,
                    green: 0,
                    blue: 0
                };
                self.plan.frame_buffer_count * self.pixel_count
            ]
            .into_boxed_slice(),
            effect_samples: vec![
                CachedEffectSample {
                    pixel_count: 0,
                    color: Color {
                        red: 0,
                        green: 0,
                        blue: 0
                    }
                };
                self.effects
                    .iter()
                    .map(|effect| self.targets[effect.target].sample_count)
                    .max()
                    .unwrap_or(0)
            ],
            effect_automation: self
                .effects
                .iter()
                .filter_map(PreparedEffect::automation_workspace)
                .collect(),
            operator_automation: self
                .plan
                .nodes
                .iter()
                .filter_map(|node| match &node.kind {
                    PreparedSignalKind::Operator {
                        operator,
                        automation,
                        ..
                    } if !automation.is_empty() => Some(EffectAutomationWorkspace {
                        params: operator.params.clone(),
                        plan: automation.clone(),
                        sample_time: None,
                    }),
                    _ => None,
                })
                .collect(),
        };
        for effect in self.effects.iter() {
            let program = effect.implementation.dsl_program();
            workspace
                .effect_vm
                .reserve(self.sample_program(program).bytecode());
        }
        for node in self.plan.nodes.iter() {
            let PreparedSignalKind::Operator {
                operator:
                    PreparedOperatorNode {
                        implementation: PreparedOperator::Dsl(program),
                        ..
                    },
                vm_slot,
                ..
            } = &node.kind
            else {
                continue;
            };
            workspace.operator_vm[*vm_slot]
                .0
                .reserve(self.operator_program(*program).bytecode());
        }
        workspace
    }
}

impl<P, E, A> PreparedSignalGraph<P, E, A> {
    /// Replace the two storage banks without cloning graph metadata.
    pub(crate) fn map_storage<Q, F>(
        self,
        map: impl FnOnce(P, Box<[E]>) -> (Q, Box<[F]>),
    ) -> PreparedSignalGraph<Q, F, A> {
        let (programs, parameter_environments) = map(self.programs, self.parameter_environments);
        PreparedSignalGraph {
            programs,
            parameter_environments,
            frame_rate: self.frame_rate,
            frame_count: self.frame_count,
            duration: self.duration,
            fixtures: self.fixtures,
            fixture_pixel_offsets: self.fixture_pixel_offsets,
            pixel_count: self.pixel_count,
            effects: self.effects,
            clips: self.clips,
            targets: self.targets,
            target_pixels: self.target_pixels,
            spatial_contexts: self.spatial_contexts,
            effects_by_layer: self.effects_by_layer,
            layers: self.layers,
            plan: self.plan,
        }
    }

    /// Consume raw/typed automation once while retaining the other graph banks.
    pub(crate) fn try_map_automation<B, X>(
        self,
        mut effect: impl FnMut(PreparedEffect<A>) -> Result<PreparedEffect<B>, X>,
        mut node: impl FnMut(PreparedSignalNode<A>) -> Result<PreparedSignalNode<B>, X>,
    ) -> Result<PreparedSignalGraph<P, E, B>, X> {
        let effects = self
            .effects
            .into_vec()
            .into_iter()
            .map(&mut effect)
            .collect::<Result<_, _>>()?;
        let plan = SignalPlan {
            nodes: self
                .plan
                .nodes
                .into_vec()
                .into_iter()
                .map(&mut node)
                .collect::<Result<_, _>>()?,
            output_index: self.plan.output_index,
            target: self.plan.target,
            vm_workspace_count: self.plan.vm_workspace_count,
            frame_nodes: self.plan.frame_nodes,
            frame_slots: self.plan.frame_slots,
            frame_buffer_count: self.plan.frame_buffer_count,
        };
        Ok(PreparedSignalGraph {
            programs: self.programs,
            parameter_environments: self.parameter_environments,
            frame_rate: self.frame_rate,
            frame_count: self.frame_count,
            duration: self.duration,
            fixtures: self.fixtures,
            fixture_pixel_offsets: self.fixture_pixel_offsets,
            pixel_count: self.pixel_count,
            effects,
            clips: self.clips,
            targets: self.targets,
            target_pixels: self.target_pixels,
            spatial_contexts: self.spatial_contexts,
            effects_by_layer: self.effects_by_layer,
            layers: self.layers,
            plan,
        })
    }

    fn frame_scratch_count_with(&self, frame_cache_count: impl Fn(usize) -> usize) -> usize {
        let samples_frames = |node: &PreparedSignalNode<A>| match &node.kind {
            PreparedSignalKind::Operator { operator, .. } => match operator.implementation {
                PreparedOperator::Dsl(program) => frame_cache_count(program) != 0,
            },
            _ => false,
        };
        if !self.plan.nodes.iter().any(samples_frames) {
            return 0;
        }
        let mut depths = Vec::with_capacity(self.plan.nodes.len());
        let mut required = 0;
        for node in &self.plan.nodes {
            let (inputs, extra) = match &node.kind {
                PreparedSignalKind::Layer { .. } => (&[][..], 0),
                PreparedSignalKind::Operator { inputs, .. } => (&inputs[..], 0),
                PreparedSignalKind::Output { inputs } => {
                    (&inputs[..], usize::from(inputs.len() > 1))
                }
            };
            let depth = inputs.iter().map(|&index| depths[index]).max().unwrap_or(0) + extra;
            depths.push(depth);
            if samples_frames(node) {
                required = required.max(depth);
            }
        }
        required
    }

    pub(crate) fn target(&self, index: usize) -> &[PreparedPixel] {
        let range = &self.targets[index].pixels;
        &self.target_pixels[range.start..range.end]
    }

    pub(crate) fn active_effect_count(&self, sample_time: SampleTime) -> usize {
        self.effects
            .iter()
            .filter(|effect| effect.is_active(sample_time))
            .count()
    }
}

impl SignalGraph<'_> {
    pub(crate) fn spatial_context(
        &self,
        uses_spatial: bool,
        pixel: usize,
    ) -> &crate::dsl::SpatialContext {
        // Admission proves this capability cannot read geometry. This value is
        // only the unused argument required by the VM's uniform execution ABI.
        const UNUSED: crate::dsl::SpatialContext = crate::dsl::SpatialContext {
            position: [0.0; 2],
            min: [0.0; 2],
            max: [0.0; 2],
        };
        if uses_spatial {
            &self.data.spatial_contexts[pixel]
        } else {
            &UNUSED
        }
    }

    /// Returns the rendered colors in the owned workspace until its next evaluation.
    pub(crate) fn evaluate<'a>(
        &self,
        sample_time: SampleTime,
        workspace: &'a mut EvaluationWorkspace,
    ) -> &'a [Color] {
        if sample_time.as_ticks() >= self.duration.as_ticks() {
            let range = crate::evaluation::frame_range(*self, self.plan.output_index);
            let output = &mut workspace.signal_buffers[range];
            output.fill(Color {
                red: 0,
                green: 0,
                blue: 0,
            });
            return output;
        }
        crate::evaluation::sample_signal_graph(*self, sample_time, workspace)
    }
}

fn operator_frame_cache_count(program: &OperatorProgram) -> usize {
    program
        .bytecode()
        .instructions
        .iter()
        .fold(0, |count, instruction| match instruction {
            crate::dsl::bytecode::Instruction::SignalSample { frame_cache, .. }
                if *frame_cache != u32::MAX =>
            {
                count.max(*frame_cache as usize + 1)
            }
            _ => count,
        })
}
