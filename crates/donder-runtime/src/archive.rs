//! Portable prepared-sequence archives. The format uses 32-bit little-endian
//! fields; rkyv owns pointer relocation, sharing, and structural archive validation.
//! Semantic validity is trusted to the Donder producer.

use crate::sequence::{PreparedSequence, SequenceData};
use alloc::{boxed::Box, vec, vec::Vec};
use rkyv::Archived;

pub const HEADER_BYTES: usize = 16;
const MAGIC: [u8; 4] = *b"DOND";
/// Current prepared-sequence format accepted by this runtime.
pub const FORMAT_VERSION: u32 = 49;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    Header,
    Version,
    Checksum,
    Archive,
    Limit,
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
    check_resource_limits(&data, limits)?;
    PreparedSequence::from_archive(data)
}

/// Estimate playback storage using references and layouts supplied by the trusted
/// producer. This checks resource budgets, not graph or program validity.
fn check_resource_limits(sequence: &SequenceData, limits: LoadLimits) -> Result<(), LoadError> {
    use crate::dsl::{AutomationPlan, BatchWorkspace};
    use crate::signal::{
        CachedEffectSample, CachedSignalFrame, EffectAutomationWorkspace, PreparedOperatorNode,
        PreparedSignalKind,
    };
    use crate::values::Color;

    let signal = &sequence.signals;
    let plan = &signal.plan;
    let mut workspace = 0usize;
    let mut reserve = |count: usize, width: usize| -> Result<(), LoadError> {
        workspace = workspace
            .checked_add(count.checked_mul(width).ok_or(LoadError::Limit)?)
            .ok_or(LoadError::Limit)?;
        if workspace > limits.workspace_bytes {
            return Err(LoadError::Limit);
        }
        Ok(())
    };
    reserve(
        signal.pixel_count,
        plan.frame_buffer_count
            .checked_mul(size_of::<Color>())
            .ok_or(LoadError::Limit)?,
    )?;
    // Every batch workspace (effects plus one per operator depth) reserves the
    // component-wise largest layout it can execute. Budgeting that maximum for
    // every slot also covers a program reused by several operators.
    let mut layout = crate::dsl::bytecode::SlotLayout::default();
    let mut array_capacity = 0usize;
    let mut array_width = 0usize;
    let mut loop_count = 0u32;
    for program in &signal.programs {
        let program_layout = program.layout;
        if program_layout.exceeded_bank().is_some() {
            return Err(LoadError::Archive);
        }
        for (maximum, count) in [
            (&mut layout.ints, program_layout.ints),
            (&mut layout.floats, program_layout.floats),
            (&mut layout.bools, program_layout.bools),
            (&mut layout.colors, program_layout.colors),
            (&mut layout.arrays, program_layout.arrays),
            (&mut layout.enums, program_layout.enums),
            (&mut layout.marks, program_layout.marks),
            (&mut layout.curves, program_layout.curves),
            (&mut layout.gradients, program_layout.gradients),
        ] {
            *maximum = (*maximum).max(count);
        }
        array_capacity = array_capacity.max(program.array_capacity as usize);
        array_width = array_width.max(program.array_width as usize);
        loop_count = loop_count.max(program.loop_count);
    }
    reserve(plan.vm_workspace_count, size_of::<Vec<CachedSignalFrame>>())?;
    reserve(
        plan.vm_workspace_count
            .checked_add(1)
            .ok_or(LoadError::Limit)?,
        BatchWorkspace::storage_estimate(layout, loop_count, array_capacity, array_width)
            .ok_or(LoadError::Limit)?,
    )?;
    let mut operator_frame_counts = vec![0usize; plan.vm_workspace_count];
    for node in &plan.nodes {
        let PreparedSignalKind::Operator {
            operator: PreparedOperatorNode { program, .. },
            vm_slot,
            ..
        } = &node.kind
        else {
            continue;
        };
        let slot = &mut operator_frame_counts[*vm_slot];
        *slot = (*slot).max(signal.programs[*program].frame_cache_count());
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
        signal
            .targets
            .iter()
            .map(|target| target.sample_count)
            .max()
            .unwrap_or(0),
        size_of::<CachedEffectSample>(),
    )?;
    for effect in &signal.effects {
        if let Some(automation) = &effect.automation {
            reserve(1, size_of::<EffectAutomationWorkspace>())?;
            reserve(
                1,
                AutomationPlan::storage_estimate(&automation.bindings).ok_or(LoadError::Limit)?,
            )?;
            reserve(
                1,
                effect
                    .bound_params
                    .automation_storage_estimate(&automation.bindings)
                    .ok_or(LoadError::Limit)?,
            )?;
        }
    }
    for node in &plan.nodes {
        if let PreparedSignalKind::Operator {
            operator,
            automation,
            ..
        } = &node.kind
            && !automation.is_empty()
        {
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
        }
    }
    reserve(
        signal.frame_scratch_count(),
        signal
            .pixel_count
            .checked_mul(size_of::<Color>())
            .and_then(|bytes| bytes.checked_add(size_of::<Box<[Color]>>()))
            .ok_or(LoadError::Limit)?,
    )?;
    reserve(1, size_of::<crate::signal::EvaluationWorkspace>())?;
    reserve(sequence.outputs.len(), size_of::<Box<[u8]>>())?;
    for output in &sequence.outputs {
        reserve(output.width, 1)?;
    }
    Ok(())
}
