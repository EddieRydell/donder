use crate::automation::AutomationMapping;
use crate::dsl::bytecode::BytecodeProgram;
use crate::dsl::{BoundParams, RuntimeError, VmWorkspace};
use crate::native_effect::NativeSample;
use crate::values::{
    Color, Curve, SampleDuration, SampleTime, SampleTimeError, sample_time_from_frame,
};
use crate::{BuiltinEffect, BuiltinOperator};
use alloc::boxed::Box;
#[cfg(not(feature = "atomic"))]
use alloc::rc::Rc as Arc;
use alloc::string::String;
#[cfg(feature = "atomic")]
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

pub use crate::evaluation::{EffectSampler, apply_bound_automation};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvaluationError {
    InvalidTiming { reason: String },
    InvalidGraph { message: String },
    Vm { message: String },
    InvalidWorkspace,
}

impl From<RuntimeError> for EvaluationError {
    fn from(error: RuntimeError) -> Self {
        Self::Vm {
            message: error.message,
        }
    }
}

/// Frozen effects, operators, targets, and execution plan; evaluates logical colors.
#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedSignalGraph {
    pub parameter_environments: Box<[crate::bindings::PreparedParameterEnvironment]>,
    pub workspace_key: u32,
    pub frame_rate: u32,
    pub frame_count: u32,
    #[rkyv(with = crate::wire::Microseconds)]
    pub duration: SampleDuration,
    pub elements: Box<[PreparedElement]>,
    pub element_cell_offsets: Box<[usize]>,
    pub pixel_count: usize,
    pub effects: Box<[PreparedEffect]>,
    pub programs: Box<[BytecodeProgram]>,
    pub targets: Box<[PreparedTarget]>,
    pub target_pixels: Box<[PreparedPixel]>,
    pub effects_by_layer: Box<[Box<[usize]>]>,
    pub layers: Box<[PreparedLayer]>,
    pub plan: SignalPlan,
}

#[derive(Clone, Copy, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedElement {
    pub id: u32,
    pub pixel_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvaluatedFrame {
    pub frame_index: u32,
    pub frame_rate: u32,
    pub sample_time: SampleTime,
    pub elements: Vec<EvaluatedElement>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvaluatedElement {
    pub element_id: u32,
    pub pixels: Vec<Color>,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedEffect {
    #[rkyv(with = crate::wire::Microseconds)]
    pub start_time: SampleTime,
    #[rkyv(with = crate::wire::Microseconds)]
    pub duration: SampleDuration,
    pub target: u32,
    pub implementation: PreparedEffectImplementation,
    pub automation: Option<Box<PreparedEffectAutomation>>,
}

impl PreparedEffect {
    pub fn is_active(&self, sample_time: SampleTime) -> bool {
        sample_time >= self.start_time
            && self
                .start_time
                .checked_add_duration(self.duration)
                .is_some_and(|end| sample_time < end)
    }

    pub fn local_time(&self, sample_time: SampleTime) -> SampleDuration {
        sample_time
            .checked_duration_since(self.start_time)
            .unwrap_or(SampleDuration::from_ticks(0))
    }

    pub fn progress(&self, sample_time: SampleTime) -> f32 {
        let elapsed = sample_time
            .checked_duration_since(self.start_time)
            .map_or(0, |duration| duration.as_ticks());
        (elapsed as f32 / self.duration.as_ticks() as f32).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PreparedEffectImplementation {
    Bound {
        environment: u32,
        implementation: BoundEffectImplementation,
    },
    Dsl {
        program: u32,
        bound_params: BoundParams,
    },
    Native {
        sample: NativeSample,
        params: Option<(BuiltinEffect, BoundParams)>,
    },
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum BoundEffectImplementation {
    Dsl(u32),
    Native(crate::native_effect::NativeParameterSample),
}

impl PreparedEffectImplementation {
    pub fn dsl_program(&self) -> Option<u32> {
        match self {
            Self::Dsl { program, .. }
            | Self::Bound {
                implementation: BoundEffectImplementation::Dsl(program),
                ..
            } => Some(*program),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedLayer {
    pub enabled: bool,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedEffectAutomation {
    /// Dense index in automated-effect order, assigned by elaboration.
    pub workspace_slot: u32,
    pub bindings: Box<[PreparedAutomation]>,
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
pub struct SignalPlan {
    pub output_index: usize,
    pub target: u32,
    pub nodes: Box<[PreparedSignalNode]>,
    pub vm_workspace_count: usize,
    pub frame_nodes: Box<[usize]>,
    pub frame_slots: Box<[u16]>,
    pub frame_buffer_count: u16,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedSignalNode {
    pub kind: PreparedSignalKind,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PreparedSignalKind {
    Layer {
        layer_index: usize,
    },
    Operator {
        operator: PreparedOperatorNode,
        inputs: Box<[usize]>,
        automation: Box<[PreparedAutomation]>,
        vm_slot: u16,
    },
    Output {
        inputs: Box<[usize]>,
    },
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PreparedOperator {
    Native(BuiltinOperator),
    Dsl(u32),
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedOperatorNode {
    /// Dense index among automated graph nodes; unused without bindings.
    pub automation_slot: u32,
    pub implementation: PreparedOperator,
    pub params: BoundParams,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedTarget {
    pub pixels: core::ops::Range<u32>,
    /// Zero disables sample reuse; otherwise this is the required cache width.
    pub sample_count: u32,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedPixel {
    pub element_index: u16,
    pub element_cell_index: u16,
    pub pixel_index: u32,
    pub pixel_count: u32,
    pub pixel_fraction: f32,
}

impl PreparedPixel {
    pub fn try_new(
        element_index: usize,
        element_cell_index: usize,
        pixel_index: usize,
        pixel_count: usize,
        pixel_fraction: f32,
    ) -> Option<Self> {
        Some(Self {
            element_index: u16::try_from(element_index).ok()?,
            element_cell_index: u16::try_from(element_cell_index).ok()?,
            pixel_index: u32::try_from(pixel_index).ok()?,
            pixel_count: u32::try_from(pixel_count).ok()?,
            pixel_fraction,
        })
    }

    pub fn element_index(&self) -> usize {
        self.element_index as usize
    }

    pub fn element_cell_index(&self) -> usize {
        self.element_cell_index as usize
    }

    pub fn pixel_index(&self) -> usize {
        self.pixel_index as usize
    }

    pub fn pixel_count(&self) -> usize {
        self.pixel_count as usize
    }
}

#[derive(Debug)]
pub struct EvaluationWorkspace {
    pub(crate) parameters: crate::bindings::ParameterWorkspace,
    pub(crate) effect_vm: VmWorkspace,
    pub(crate) effect_vm_sample: Option<(CachedVmSample, SampleDuration, Color)>,
    pub(crate) operator_vm: Vec<(VmWorkspace, Option<CachedVmSample>)>,
    pub(crate) operator_frames: Vec<Vec<CachedSignalFrame>>,
    pub(crate) signal_cache: Box<[Option<CachedSignal>]>,
    pub(crate) signal_buffers: Box<[Color]>,
    pub(crate) frame_scratch: Vec<Box<[Color]>>,
    pub(crate) effect_samples: Vec<CachedEffectSample>,
    pub(crate) effect_automation: Vec<EffectAutomationWorkspace>,
    pub(crate) operator_automation: Vec<(BoundParams, Option<SampleTime>)>,
    pub(crate) workspace_key: Option<u32>,
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
    pub(crate) pixel_count: u32,
    pub(crate) color: Color,
}

#[derive(Debug, Default)]
pub struct EffectAutomationWorkspace {
    pub(crate) params: Option<BoundParams>,
    pub(crate) sample_time: Option<SampleTime>,
}

impl PreparedSignalGraph {
    /// Keep independent temporal query sites resident while visiting pixels.
    /// Looping sites can replace their own old requested times in this bounded
    /// cache; identities always include the exact requested SampleTime.
    pub(crate) fn parameter_time_slots(&self) -> usize {
        if self.parameter_environments.is_empty() {
            return 0;
        }
        let mut slots = 1usize;
        for node in &self.plan.nodes {
            if let PreparedSignalKind::Operator { operator, .. } = &node.kind {
                slots = slots.saturating_add(match operator.implementation {
                    PreparedOperator::Dsl(program) => {
                        self.programs.get(program as usize).map_or(0, |program| {
                            program
                                .instructions
                                .iter()
                                .filter(|instruction| {
                                    matches!(
                                        instruction,
                                        crate::dsl::bytecode::Instruction::SignalSample { .. }
                                    )
                                })
                                .count()
                        })
                    }
                    PreparedOperator::Native(builtin) => usize::from(builtin.resamples_time()),
                });
            }
        }
        slots
    }
    /// Maximum number of temporary frames held by nested whole-frame sampling.
    /// DSL frame caches own their storage separately, but may sample native inputs.
    pub(crate) fn frame_scratch_count(&self) -> usize {
        let samples_frames = |node: &PreparedSignalNode| match &node.kind {
            PreparedSignalKind::Operator { operator, .. } => match operator.implementation {
                PreparedOperator::Native(builtin) => builtin.resamples_time(),
                PreparedOperator::Dsl(program) => {
                    self.programs[program as usize].frame_cache_count() != 0
                }
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
                PreparedSignalKind::Operator {
                    inputs, operator, ..
                } => (
                    &inputs[..],
                    match operator.implementation {
                        PreparedOperator::Native(builtin) => builtin.scratch_frames(),
                        PreparedOperator::Dsl(_) => 0,
                    },
                ),
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

    pub fn target(&self, index: u32) -> &[PreparedPixel] {
        let range = &self.targets[index as usize].pixels;
        &self.target_pixels[range.start as usize..range.end as usize]
    }

    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    pub fn frame_rate(&self) -> u32 {
        self.frame_rate
    }

    pub fn pixel_count(&self) -> usize {
        self.pixel_count
    }

    pub fn duration(&self) -> SampleDuration {
        self.duration
    }

    /// Preallocates frame buffers, VM registers, calculated-array slots,
    /// and automation storage.
    pub fn workspace(&self) -> EvaluationWorkspace {
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
            let count = self.programs[*program as usize].frame_cache_count();
            operator_frame_counts[usize::from(*vm_slot)] =
                operator_frame_counts[usize::from(*vm_slot)].max(count);
        }
        let mut workspace = EvaluationWorkspace {
            parameters: crate::bindings::ParameterWorkspace::new(
                &self.parameter_environments,
                self.parameter_time_slots(),
            ),
            effect_vm: VmWorkspace::default(),
            frame_scratch: (0..self.frame_scratch_count())
                .map(|_| vec![crate::element::black(); self.pixel_count].into_boxed_slice())
                .collect(),
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
                    match &self.plan.nodes[index].kind {
                        PreparedSignalKind::Operator { operator, .. } => {
                            match operator.implementation {
                                PreparedOperator::Dsl(_) => true,
                                PreparedOperator::Native(builtin) => builtin.resamples_time(),
                            }
                        }
                        _ => false,
                    }
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
                usize::from(self.plan.frame_buffer_count) * self.pixel_count
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
                    .map(|effect| self.targets[effect.target as usize].sample_count as usize)
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
                    } if !automation.is_empty() => {
                        Some((automation_params(&operator.params, automation), None))
                    }
                    _ => None,
                })
                .collect(),
            workspace_key: Some(self.workspace_key),
        };
        for effect in self.effects.iter() {
            if let Some(program) = effect.implementation.dsl_program() {
                workspace
                    .effect_vm
                    .reserve(&self.programs[program as usize]);
            }
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
            workspace.operator_vm[usize::from(*vm_slot)]
                .0
                .reserve(&self.programs[*program as usize]);
        }
        workspace
    }

    /// Returns the rendered colors in the workspace, valid until its next evaluation.
    pub fn evaluate<'a>(
        &self,
        sample_time: SampleTime,
        workspace: &'a mut EvaluationWorkspace,
    ) -> Result<&'a [Color], EvaluationError> {
        if workspace.workspace_key != Some(self.workspace_key) {
            return Err(EvaluationError::InvalidWorkspace);
        }
        if sample_time.as_ticks() >= self.duration.as_ticks() {
            let range = crate::evaluation::frame_range(self, self.plan.output_index)?;
            let output = &mut workspace.signal_buffers[range];
            output.fill(Color {
                red: 0,
                green: 0,
                blue: 0,
            });
            return Ok(output);
        }
        crate::evaluation::sample_signal_graph(self, sample_time, workspace)
    }

    pub fn evaluate_frame(&self, frame_index: u32) -> Result<EvaluatedFrame, EvaluationError> {
        self.evaluate_frame_with_workspace(frame_index, &mut self.workspace())
    }

    pub fn evaluate_frame_with_workspace(
        &self,
        frame_index: u32,
        workspace: &mut EvaluationWorkspace,
    ) -> Result<EvaluatedFrame, EvaluationError> {
        let sample_time =
            sample_time_from_frame(frame_index, self.frame_rate).map_err(sample_time_error)?;
        self.evaluate_elements(frame_index, sample_time, workspace)
    }

    fn evaluate_elements(
        &self,
        frame_index: u32,
        sample_time: SampleTime,
        workspace: &mut EvaluationWorkspace,
    ) -> Result<EvaluatedFrame, EvaluationError> {
        let colors = self.evaluate(sample_time, workspace)?;
        let mut offset = 0;
        let elements = self
            .elements
            .iter()
            .map(|element| {
                let end = offset + element.pixel_count;
                let pixels = colors[offset..end].to_vec();
                offset = end;
                EvaluatedElement {
                    element_id: element.id,
                    pixels,
                }
            })
            .collect();
        Ok(EvaluatedFrame {
            frame_index,
            frame_rate: self.frame_rate,
            sample_time,
            elements,
        })
    }

    pub fn active_effect_count(&self, sample_time: SampleTime) -> usize {
        self.effects
            .iter()
            .filter(|effect| effect.is_active(sample_time))
            .count()
    }

    pub fn active_effect_count_at_frame(&self, frame_index: u32) -> usize {
        sample_time_from_frame(frame_index, self.frame_rate)
            .map(|time| self.active_effect_count(time))
            .unwrap_or(0)
    }
}

pub(crate) fn automation_params(
    params: &BoundParams,
    automation: &[PreparedAutomation],
) -> BoundParams {
    let mut params = params.clone_for_automation();
    for binding in automation {
        params.reserve_automation(
            usize::from(binding.param_index),
            &binding.curve,
            &binding.mapping,
        );
    }
    params
}

fn sample_time_error(error: SampleTimeError) -> EvaluationError {
    EvaluationError::InvalidTiming {
        reason: alloc::format!("invalid sample time: {error:?}"),
    }
}
