//! Portable prepared-sequence archives. The format uses 32-bit little-endian
//! fields; rkyv owns pointer relocation, sharing, and archive validation.

use crate::sequence::{PreparedSequence, SequenceData};
use crate::values::{SampleDuration, SampleTime};
use alloc::{boxed::Box, vec, vec::Vec};
use rkyv::rancor::Fallible;
use rkyv::with::{ArchiveWith, DeserializeWith, SerializeWith};
use rkyv::{Archive, Archived, Place};

pub const HEADER_BYTES: usize = 16;
const MAGIC: [u8; 4] = *b"DOND";
/// Current prepared-sequence format accepted by this runtime.
pub const FORMAT_VERSION: u32 = 36;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    Header,
    Version,
    Checksum,
    Archive,
    Limit,
    InvalidSequence,
}

/// Admission limits for uploads from a trusted Donder compiler. Workspace bytes
/// are a conservative admission estimate, not a fallible allocator or sandbox.
/// Decoding allocates owned data; callers must also reserve heap for that data,
/// archive validation, the upload buffer, and their other tasks.
#[derive(Clone, Copy, Debug)]
pub struct LoadLimits {
    pub payload_bytes: usize,
    pub pixels: usize,
    pub graph_nodes: usize,
    pub workspace_bytes: usize,
}

impl Default for LoadLimits {
    fn default() -> Self {
        Self {
            payload_bytes: 256 * 1024,
            pixels: 4096,
            graph_nodes: 256,
            workspace_bytes: 128 * 1024,
        }
    }
}

pub fn encode_sequence(sequence: &PreparedSequence) -> Result<Vec<u8>, LoadError> {
    let payload = rkyv::to_bytes::<rkyv::rancor::Failure>(&sequence.archive_data())
        .map_err(|_| LoadError::Archive)?;
    let length = u32::try_from(payload.len()).map_err(|_| LoadError::Limit)?;
    let mut bytes = Vec::with_capacity(HEADER_BYTES + payload.len());
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(&crc32fast::hash(&payload).to_le_bytes());
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Reads only the fixed header, so transports can limit an upload before allocating it.
pub fn payload_length(header: &[u8], limits: LoadLimits) -> Result<usize, LoadError> {
    let header: &[u8; HEADER_BYTES] = header.try_into().map_err(|_| LoadError::Header)?;
    if header[..4] != MAGIC {
        return Err(LoadError::Header);
    }
    let word = |offset| {
        u32::from_le_bytes([
            header[offset],
            header[offset + 1],
            header[offset + 2],
            header[offset + 3],
        ])
    };
    if word(4) != FORMAT_VERSION {
        return Err(LoadError::Version);
    }
    let length = word(8) as usize;
    if length > limits.payload_bytes {
        return Err(LoadError::Limit);
    }
    Ok(length)
}

/// Validate the archive in place before allocating decoded playback state.
fn validate_archive(
    bytes: &[u8],
    limits: LoadLimits,
) -> Result<&Archived<SequenceData>, LoadError> {
    use rkyv::validation::{Validator, archive::ArchiveValidator, shared::SharedValidator};
    let header = bytes.get(..HEADER_BYTES).ok_or(LoadError::Header)?;
    let length = payload_length(header, limits)?;
    let payload = bytes.get(HEADER_BYTES..).ok_or(LoadError::Header)?;
    if payload.len() != length {
        return Err(LoadError::Header);
    }
    if crc32fast::hash(payload).to_le_bytes() != header[12..16] {
        return Err(LoadError::Checksum);
    }
    let mut validator = Validator::new(
        ArchiveValidator::with_max_depth(payload, core::num::NonZeroUsize::new(64)),
        SharedValidator::new(),
    );
    let archived =
        rkyv::api::access_with_context::<Archived<SequenceData>, _, rkyv::rancor::Failure>(
            payload,
            &mut validator,
        )
        .map_err(|_| LoadError::Archive)?;
    if archived.signals.pixel_count.to_native() as usize > limits.pixels
        || archived.signals.plan.nodes.len() > limits.graph_nodes
        || archived.signals.plan.vm_workspace_count.to_native() as usize
            > archived.signals.plan.nodes.len()
        || archived.signals.plan.frame_buffer_count.to_native() as usize
            > archived.signals.plan.nodes.len()
    {
        return Err(LoadError::Limit);
    }
    drop(validator);
    Ok(archived)
}

pub fn decode_sequence(bytes: &[u8], limits: LoadLimits) -> Result<PreparedSequence, LoadError> {
    let archived = validate_archive(bytes, limits)?;
    let data = rkyv::deserialize::<SequenceData, rkyv::rancor::Failure>(archived)
        .map_err(|_| LoadError::Archive)?;
    PreparedSequence::admit_data(data, Some(limits))
}

pub(crate) fn validate_sequence(
    sequence: &SequenceData,
    limits: Option<LoadLimits>,
) -> Result<(), LoadError> {
    let signal = &sequence.signals;
    let mut workspace = validate_signal_graph(signal, limits)?;
    let mut reserve = |count: usize, width: usize| -> Result<(), LoadError> {
        workspace = workspace
            .checked_add(count.checked_mul(width).ok_or(LoadError::Limit)?)
            .ok_or(LoadError::Limit)?;
        if limits.is_some_and(|limits| workspace > limits.workspace_bytes) {
            return Err(LoadError::Limit);
        }
        Ok(())
    };
    reserve(1, size_of::<crate::signal::EvaluationWorkspace>())?;
    reserve(sequence.outputs.len(), size_of::<Box<[u8]>>())?;
    for output in &sequence.outputs {
        reserve(output.width, 1)?;
    }
    let bad = LoadError::InvalidSequence;
    for route in &sequence.patch.routes {
        if route.pixels.start > route.pixels.end
            || route.pixels.end > signal.pixel_count
            || !route.encoding.is_valid()
            || route
                .lookup
                .is_some_and(|index| index >= sequence.patch.lookups.len())
        {
            return Err(bad);
        }
        let width = (route.pixels.end - route.pixels.start)
            .checked_mul(route.encoding.channel_order().len())
            .ok_or(LoadError::Limit)?;
        let end = route
            .start_slot
            .checked_add(width)
            .ok_or(LoadError::Limit)?;
        if sequence
            .outputs
            .get(route.frame)
            .is_none_or(|output| end > output.width)
        {
            return Err(bad);
        }
    }
    Ok(())
}

fn validate_signal_graph(
    signal: &crate::signal::PreparedSignalGraph,
    limits: Option<LoadLimits>,
) -> Result<usize, LoadError> {
    use crate::dsl::bytecode::{ParameterKind, ProgramContext};
    use crate::dsl::{AutomationPlan, VmWorkspace};
    use crate::signal::{
        CachedEffectSample, CachedSignal, CachedSignalFrame, CachedVmSample,
        EffectAutomationWorkspace,
    };
    use crate::signal::{
        PreparedEffectImplementation, PreparedOperator, PreparedOperatorNode, PreparedSignalKind,
    };
    use crate::values::Color;
    let bad = LoadError::InvalidSequence;
    let plan = &signal.plan;
    if signal.frame_rate == 0
        || signal.duration.as_ticks() == 0
        || signal.frame_count == 0
        || plan.output_index >= plan.nodes.len()
        || plan.target >= signal.targets.len()
        || plan.vm_workspace_count > plan.nodes.len()
        || plan.frame_buffer_count > plan.nodes.len()
        || signal.fixture_pixel_offsets.len() != signal.fixtures.len()
        || signal.layers.len() != signal.effects_by_layer.len()
        || plan.frame_slots.len() != plan.nodes.len()
    {
        return Err(bad);
    }
    if (!signal.spatial_contexts.is_empty()
        || signal
            .programs
            .iter()
            .any(|program| program.uses_spatial_context()))
        && signal.spatial_contexts.len() != signal.target_pixels.len()
    {
        return Err(bad);
    }
    for spatial in &signal.spatial_contexts {
        for axis in 0..2 {
            if !spatial.position[axis].is_finite()
                || !spatial.min[axis].is_finite()
                || !spatial.max[axis].is_finite()
                || spatial.position[axis] < spatial.min[axis]
                || spatial.position[axis] > spatial.max[axis]
            {
                return Err(bad);
            }
        }
    }
    let mut workspace = 0usize;
    let mut reserve = |count: usize, width: usize| -> Result<(), LoadError> {
        workspace = workspace
            .checked_add(count.checked_mul(width).ok_or(LoadError::Limit)?)
            .ok_or(LoadError::Limit)?;
        if limits.is_some_and(|limits| workspace > limits.workspace_bytes) {
            return Err(LoadError::Limit);
        }
        Ok(())
    };
    crate::bindings::PreparedParameterEnvironment::validate_all(&signal.parameter_environments)
        .map_err(|_| bad)?;
    reserve(
        1,
        crate::bindings::ParameterWorkspace::storage_estimate(
            &signal.parameter_environments,
            signal.parameter_time_slots()?,
        )
        .ok_or(LoadError::Limit)?,
    )?;
    reserve(
        signal.pixel_count,
        plan.frame_buffer_count * size_of::<Color>(),
    )?;
    // All VM slots reserve the component-wise largest layouts they can execute.
    // Budgeting that maximum for every slot also covers a program reused by
    // several operators, and array capacity/width maxima from different programs.
    let mut registers = [0usize; 12];
    let mut array_capacity = 0usize;
    let mut array_width = 0usize;
    let mut loop_count = 0usize;
    for program in &signal.programs {
        if !program.has_valid_structure() {
            return Err(bad);
        }
        let layout = program.layout;
        for (maximum, count) in registers.iter_mut().zip([
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
        ]) {
            *maximum = (*maximum).max(count as usize);
        }
        array_capacity = array_capacity.max(program.array_capacity as usize);
        array_width = array_width.max(program.array_width as usize);
        loop_count = loop_count.max(program.loop_count as usize);
        if program.frame_cache_count() > program.instructions.len() {
            return Err(bad);
        }
    }
    let mut operator_frame_counts = vec![0usize; plan.vm_workspace_count];
    for node in &plan.nodes {
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
        let Some(slot) = operator_frame_counts.get_mut(*vm_slot) else {
            return Err(bad);
        };
        let Some(program) = signal.programs.get(*program) else {
            return Err(bad);
        };
        *slot = (*slot).max(program.frame_cache_count());
    }
    for count in operator_frame_counts {
        reserve(
            signal.pixel_count,
            count
                .checked_mul(size_of::<Color>())
                .ok_or(LoadError::Limit)?,
        )?;
        reserve(count, size_of::<CachedSignalFrame>())?;
    }
    reserve(
        plan.vm_workspace_count,
        size_of::<Vec<CachedSignalFrame>>() + size_of::<Option<CachedVmSample>>(),
    )?;
    reserve(
        1 + plan.vm_workspace_count,
        VmWorkspace::storage_estimate(registers, array_capacity, array_width, loop_count)
            .ok_or(LoadError::Limit)?,
    )?;
    reserve(plan.nodes.len(), size_of::<Option<CachedSignal>>())?;
    let mut count = 0usize;
    for (fixture, &offset) in signal.fixtures.iter().zip(&signal.fixture_pixel_offsets) {
        if offset != count {
            return Err(bad);
        }
        count = count
            .checked_add(fixture.pixel_count)
            .ok_or(LoadError::Limit)?;
    }
    if count != signal.pixel_count {
        return Err(bad);
    }
    for target in &signal.targets {
        let pixels = signal
            .target_pixels
            .get(target.pixels.start..target.pixels.end)
            .ok_or(bad)?;
        let mut previous = None;
        for pixel in pixels {
            let fixture = signal.fixtures.get(pixel.fixture_index).ok_or(bad)?;
            let address = (pixel.fixture_index, pixel.fixture_pixel_index);
            if pixel.fixture_pixel_index as usize >= fixture.pixel_count
                || pixel.pixel_count == 0
                || pixel.pixel_index >= pixel.pixel_count
                || (target.sample_count != 0 && pixel.pixel_index >= target.sample_count)
                || !pixel.pixel_fraction.is_finite()
                || previous.is_some_and(|old| old >= address)
            {
                return Err(bad);
            }
            previous = Some(address);
        }
    }
    if signal.target(plan.target).len() != signal.pixel_count {
        return Err(bad);
    }
    let samples_local_pixels = plan.nodes.iter().any(|node| {
        let PreparedSignalKind::Operator { operator, .. } = &node.kind else {
            return false;
        };
        let PreparedOperator::Dsl(program) = operator.implementation;
        signal.programs.get(program).is_some_and(|program| {
            program.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    crate::dsl::bytecode::Instruction::SignalSample {
                        pixel: crate::dsl::bytecode::SignalPixel::Local(_),
                        ..
                    }
                )
            })
        })
    });
    // Local queries translate fixture-relative coordinates into flat storage.
    // Compaction may preserve sparse authored coordinates only when no operator
    // can issue such a query; otherwise the whole fixture must remain resident.
    if samples_local_pixels
        && signal.target(plan.target).iter().any(|pixel| {
            pixel.pixel_index != pixel.fixture_pixel_index as usize
                || pixel.pixel_count != signal.fixtures[pixel.fixture_index].pixel_count
        })
    {
        return Err(bad);
    }
    reserve(
        signal
            .targets
            .iter()
            .map(|target| target.sample_count)
            .max()
            .unwrap_or(0),
        size_of::<CachedEffectSample>(),
    )?;
    for clip in &signal.clips {
        if clip.target >= signal.targets.len()
            || clip.duration.as_ticks() == 0
            || clip
                .start_time
                .checked_add_duration(clip.duration)
                .is_none_or(|end| end.as_ticks() > signal.duration.as_ticks())
            || clip
                .effects
                .iter()
                .any(|&effect| effect >= signal.effects.len())
        {
            return Err(bad);
        }
    }
    let mut automation_slot = 0;
    for effect in &signal.effects {
        if effect.target >= signal.targets.len()
            || effect.duration.as_ticks() == 0
            || effect
                .start_time
                .checked_add_duration(effect.duration)
                .is_none_or(|end| end.as_ticks() > signal.duration.as_ticks())
        {
            return Err(bad);
        }
        match &effect.implementation {
            PreparedEffectImplementation::Dsl {
                program,
                bound_params,
            } => {
                if !bound_params.is_frozen()
                    || signal.programs.get(*program).is_none_or(|program| {
                        !program.has_valid_context(ProgramContext::Effect)
                            || !program.has_valid_parameter_reads(|index| {
                                bound_params.parameter_kind(index)
                            })
                            || !program.has_valid_reference_parameter_reads(|index, expected| {
                                bound_params.parameter_accepts_type(index, expected)
                            })
                    })
                {
                    return Err(bad);
                }
            }
            PreparedEffectImplementation::Bound {
                environment,
                program,
            } => {
                if effect.automation.is_some()
                    || signal
                        .parameter_environments
                        .get(*environment)
                        .zip(signal.programs.get(*program))
                        .is_none_or(|(environment, program)| {
                            !program.has_valid_context(ProgramContext::Effect)
                                || !program.has_valid_parameter_reads(|index| {
                                    environment
                                        .output_types()
                                        .get(index)
                                        .map(ParameterKind::for_type)
                                })
                                || !program.has_valid_reference_parameter_reads(
                                    |index, expected| {
                                        environment
                                            .output_types()
                                            .get(index)
                                            .is_some_and(|actual| expected.accepts(actual))
                                    },
                                )
                        })
                {
                    return Err(bad);
                }
            }
        }
        if let Some(automation) = &effect.automation {
            reserve(1, size_of::<EffectAutomationWorkspace>())?;
            reserve(
                1,
                AutomationPlan::storage_estimate(&automation.bindings).ok_or(LoadError::Limit)?,
            )?;
            match &effect.implementation {
                PreparedEffectImplementation::Dsl { bound_params, .. } => {
                    if !bound_params.has_valid_automation(&automation.bindings) {
                        return Err(bad);
                    }
                    reserve(
                        1,
                        bound_params
                            .automation_storage_estimate(&automation.bindings)
                            .ok_or(LoadError::Limit)?,
                    )?;
                }
                _ => return Err(bad),
            }
            if automation.workspace_slot != automation_slot {
                return Err(bad);
            }
            automation_slot += 1;
        }
    }
    for layer in &signal.effects_by_layer {
        let mut previous = None;
        for &index in layer {
            let effect = signal.effects.get(index).ok_or(bad)?;
            if previous.is_some_and(|time| time > effect.start_time) {
                return Err(bad);
            }
            previous = Some(effect.start_time);
        }
    }
    let mut automation_slot = 0;
    let mut depths = Vec::with_capacity(plan.nodes.len());
    let mut nested_vm_depths = Vec::with_capacity(plan.nodes.len());
    for (index, node) in plan.nodes.iter().enumerate() {
        let (inputs, vm_slot) = match &node.kind {
            PreparedSignalKind::Layer { layer_index } => {
                if *layer_index >= signal.layers.len() {
                    return Err(bad);
                }
                (&[][..], None)
            }
            PreparedSignalKind::Operator {
                operator,
                inputs,
                automation,
                vm_slot,
            } => {
                if *vm_slot >= plan.vm_workspace_count {
                    return Err(bad);
                }
                let PreparedOperator::Dsl(program) = operator.implementation;
                if program >= signal.programs.len() {
                    return Err(bad);
                }
                if !operator.params.is_frozen()
                    || !signal.programs[program].has_valid_context(ProgramContext::Operator {
                        inputs: inputs.len(),
                    })
                    || !signal.programs[program]
                        .has_valid_parameter_reads(|index| operator.params.parameter_kind(index))
                    || !signal.programs[program].has_valid_reference_parameter_reads(
                        |index, expected| operator.params.parameter_accepts_type(index, expected),
                    )
                {
                    return Err(bad);
                }
                if !automation.is_empty() {
                    if !operator.params.has_valid_automation(automation) {
                        return Err(bad);
                    }
                    reserve(1, size_of::<EffectAutomationWorkspace>())?;
                    reserve(
                        1,
                        AutomationPlan::storage_estimate(automation).ok_or(LoadError::Limit)?,
                    )?;
                    reserve(
                        1,
                        operator
                            .params
                            .automation_storage_estimate(automation)
                            .ok_or(LoadError::Limit)?,
                    )?;
                    if operator.automation_slot != automation_slot {
                        return Err(bad);
                    }
                    automation_slot += 1;
                }
                (&inputs[..], Some(*vm_slot))
            }
            PreparedSignalKind::Output { inputs } => (&inputs[..], None),
        };
        if inputs.iter().any(|&input| input >= index) {
            return Err(bad);
        }
        let input_vm_depth = inputs
            .iter()
            .map(|&input| nested_vm_depths[input])
            .max()
            .unwrap_or(0);
        let vm_depth = if let Some(slot) = vm_slot {
            if slot < input_vm_depth {
                return Err(bad);
            }
            slot + 1
        } else {
            input_vm_depth
        };
        nested_vm_depths.push(vm_depth);
        let depth = inputs.iter().map(|&input| depths[input]).max().unwrap_or(0) + 1;
        if limits.is_some() && depth > 32 {
            return Err(LoadError::Limit);
        }
        depths.push(depth);
    }
    let mut scheduled = vec![false; plan.nodes.len()];
    let mut slot_owner = vec![None; plan.frame_buffer_count];
    for &node in &plan.frame_nodes {
        if node >= plan.nodes.len()
            || scheduled[node]
            || plan.frame_slots[node] >= plan.frame_buffer_count
        {
            return Err(bad);
        }
        let destination = plan.frame_slots[node];
        if let PreparedSignalKind::Output { inputs } = &plan.nodes[node].kind
            && inputs.iter().any(|&input| {
                let source = plan.frame_slots[input];
                !scheduled[input]
                    || source == destination
                    || slot_owner.get(source) != Some(&Some(input))
            })
        {
            return Err(bad);
        }
        slot_owner[destination] = Some(node);
        scheduled[node] = true;
    }
    reserve(
        signal.frame_scratch_count(),
        signal
            .pixel_count
            .checked_mul(size_of::<Color>())
            .and_then(|bytes| bytes.checked_add(size_of::<Box<[Color]>>()))
            .ok_or(LoadError::Limit)?,
    )?;
    if plan.frame_slots[plan.output_index] >= plan.frame_buffer_count {
        return Err(bad);
    }
    let output_owner = slot_owner[plan.frame_slots[plan.output_index]];
    if !scheduled[plan.output_index] {
        let PreparedSignalKind::Output { inputs } = &plan.nodes[plan.output_index].kind else {
            return Err(bad);
        };
        let [input] = inputs.as_ref() else {
            return Err(bad);
        };
        if !scheduled[*input]
            || plan.frame_slots[plan.output_index] != plan.frame_slots[*input]
            || output_owner != Some(*input)
        {
            return Err(bad);
        }
    } else if output_owner != Some(plan.output_index) {
        return Err(bad);
    }
    Ok(workspace)
}

pub(crate) struct Microseconds;

macro_rules! archive_clock {
    ($clock:ty) => {
        impl ArchiveWith<$clock> for Microseconds {
            type Archived = Archived<u32>;
            type Resolver = ();
            fn resolve_with(value: &$clock, _: (), out: Place<Self::Archived>) {
                value.as_ticks().resolve((), out);
            }
        }
        impl<S: Fallible + ?Sized> SerializeWith<$clock, S> for Microseconds {
            fn serialize_with(_: &$clock, _: &mut S) -> Result<(), S::Error> {
                Ok(())
            }
        }
        impl<D: Fallible + ?Sized> DeserializeWith<Archived<u32>, $clock, D> for Microseconds {
            fn deserialize_with(value: &Archived<u32>, _: &mut D) -> Result<$clock, D::Error> {
                Ok(<$clock>::from_ticks(value.to_native()))
            }
        }
    };
}
archive_clock!(SampleTime);
archive_clock!(SampleDuration);
