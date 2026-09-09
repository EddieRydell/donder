use super::elements::OutputElements;
use super::errors::SequenceOutputPrepareError;
use super::frame::ControllerPortFrame;

#[cfg(test)]
#[path = "patch_tests.rs"]
mod tests;
use dawn_language::element::ElementTree;
use dawn_language::fixture_profile::FixtureProfileStore;
use dawn_language::patch::{
    FilterDefinition, PatchGraph, PatchNode, PatchNodeId, PatchPortId, prepare_filter,
    prepare_fixture_encoding,
};
use dawn_runtime::patch::{
    ColorEncoding, PatchSource, PatchSourceSpan, PatchStep, PatchValue, PreparedFilter,
    PreparedPatch,
};
use std::collections::{HashMap, HashSet};

pub(crate) fn prepare_patch(
    tree: &ElementTree,
    patch: &PatchGraph,
    profiles: &FixtureProfileStore,
    frames: &[ControllerPortFrame],
    elements: &OutputElements,
) -> Result<PreparedPatch, SequenceOutputPrepareError> {
    let index32 = |value| {
        u32::try_from(value).map_err(|_| {
            SequenceOutputPrepareError::InvalidPatch(
                "prepared patch exceeds the 32-bit index range".to_string(),
            )
        })
    };
    let order = patch
        .validate()
        .map_err(|error| SequenceOutputPrepareError::InvalidPatch(format!("{error:?}")))?;
    let incoming = patch
        .edges
        .iter()
        .map(|edge| (edge.to, (edge.from, edge.from_port)))
        .collect::<HashMap<_, _>>();
    // Walk backward from the requested ports before lowering or assigning slots.
    // Shared sources/filters are retained once, regardless of fan-out.
    let mut required = HashSet::new();
    for (id, node) in &patch.nodes {
        if let PatchNode::Sink(sink) = node
            && frames
                .iter()
                .any(|frame| frame.controller == sink.controller && frame.port == sink.port)
        {
            let mut cursor = *id;
            while required.insert(cursor) {
                let Some((source, _)) = incoming.get(&cursor) else {
                    break;
                };
                cursor = *source;
            }
        }
    }
    let mut outputs = HashMap::<(PatchNodeId, PatchPortId), u32>::new();
    let mut value_types = Vec::new();
    let mut steps = Vec::with_capacity(order.len());
    let mut fixture_programs = Vec::new();
    let mut fixture_program_ids = HashMap::new();

    for id in order {
        if !required.contains(&id) {
            continue;
        }
        let node = patch.nodes.get(&id).ok_or_else(|| {
            SequenceOutputPrepareError::InvalidPatch("validated patch node disappeared".to_string())
        })?;
        match node {
            PatchNode::Source(source) => {
                let addresses = source
                    .validate_selection(tree)
                    .map_err(SequenceOutputPrepareError::InvalidPatch)?;
                let mut spans: Vec<PatchSourceSpan> = Vec::new();
                for address in addresses {
                    let element = *elements.indexes.get(&address.node).ok_or_else(|| {
                        SequenceOutputPrepareError::InvalidPatch(
                            "patch source references a group or missing element".to_string(),
                        )
                    })?;
                    let end = address.cell.checked_add(1).ok_or_else(|| {
                        SequenceOutputPrepareError::InvalidPatch(
                            "patch cell index exceeds span range".into(),
                        )
                    })?;
                    if let Some(last) = spans.last_mut()
                        && last.element == element
                        && last.cells.end == address.cell
                    {
                        last.cells.end = end;
                    } else {
                        spans.push(PatchSourceSpan {
                            element,
                            cells: address.cell..end,
                        });
                    }
                }
                let output = index32(value_types.len())?;
                value_types.push(source.output.clone());
                outputs.insert((id, PatchPortId(0)), output);
                steps.push(PatchStep::Source {
                    output,
                    source: PatchSource {
                        spans: spans.into_boxed_slice(),
                    },
                });
            }
            PatchNode::Filter(filter) => {
                let input = incoming
                    .get(&id)
                    .and_then(|source| outputs.get(source))
                    .copied()
                    .ok_or_else(|| {
                        SequenceOutputPrepareError::InvalidPatch(
                            "filter input is missing".to_string(),
                        )
                    })?;
                // Fan-out only names additional readers of the same immutable value.
                // Resolve it here instead of retaining buffers and copy instructions.
                if let FilterDefinition::FanOut { outputs: count, .. } = filter {
                    for port in 0..*count {
                        outputs.insert((id, PatchPortId(port)), input);
                    }
                    continue;
                }
                if let FilterDefinition::ComponentReorder {
                    components_per_cell,
                    order,
                    ..
                } = filter
                    && *components_per_cell != 0
                    && order.iter().copied().eq(0..*components_per_cell)
                {
                    outputs.insert((id, PatchPortId(0)), input);
                    continue;
                }
                let output_start = index32(value_types.len())?;
                let value_type = filter.output_type(PatchPortId(0)).ok_or_else(|| {
                    SequenceOutputPrepareError::InvalidPatch("filter output is invalid".to_string())
                })?;
                outputs.insert((id, PatchPortId(0)), output_start);
                value_types.push(value_type);
                let step = if let FilterDefinition::FixtureProfileEncoding {
                    profile,
                    slot_count,
                    ..
                } = filter
                {
                    let key = (profile.clone(), *slot_count);
                    let program = if let Some(id) = fixture_program_ids.get(&key) {
                        *id
                    } else {
                        let definition = profiles.definitions.get(profile).ok_or_else(|| {
                            SequenceOutputPrepareError::InvalidPatch(
                                "fixture profile is missing".to_string(),
                            )
                        })?;
                        let program =
                            prepare_fixture_encoding(definition, *slot_count).map_err(|error| {
                                SequenceOutputPrepareError::InvalidPatch(format!("{error:?}"))
                            })?;
                        let id = u32::try_from(fixture_programs.len()).map_err(|error| {
                            SequenceOutputPrepareError::InvalidPatch(format!("{error:?}"))
                        })?;
                        fixture_programs.push(program);
                        fixture_program_ids.insert(key, id);
                        id
                    };
                    PatchStep::Fixture {
                        input,
                        output_start,
                        program,
                    }
                } else {
                    PatchStep::Filter {
                        input,
                        output_start,
                        filter: prepare_filter(filter).map_err(|error| {
                            SequenceOutputPrepareError::InvalidPatch(format!("{error:?}"))
                        })?,
                    }
                };
                steps.push(step);
            }
            PatchNode::Sink(sink) => {
                let input = incoming
                    .get(&id)
                    .and_then(|source| outputs.get(source))
                    .copied()
                    .ok_or_else(|| {
                        SequenceOutputPrepareError::InvalidPatch(
                            "sink input is missing".to_string(),
                        )
                    })?;
                let frame = index32(
                    frames
                        .iter()
                        .position(|frame| {
                            frame.controller == sink.controller && frame.port == sink.port
                        })
                        .ok_or_else(|| {
                            SequenceOutputPrepareError::InvalidPatch(
                                "patch sink references a controller port outside the active setup"
                                    .to_string(),
                            )
                        })?,
                )?;
                let start = u32::from(sink.start_slot);
                let end = start
                    .checked_add(u32::from(sink.slot_count))
                    .ok_or_else(|| {
                        SequenceOutputPrepareError::InvalidPatch(
                            "patch sink slot range overflowed".to_string(),
                        )
                    })?;
                if end as usize > frames[frame as usize].slots.len() {
                    return Err(SequenceOutputPrepareError::InvalidPatch(
                        "patch sink exceeds its controller port".to_string(),
                    ));
                }
                steps.push(PatchStep::Sink {
                    input,
                    frame,
                    start,
                    end,
                });
            }
        }
    }

    fuse_rgb_packing(&mut steps, value_types.len())?;

    // Keep a value through its last reader, including readers reached through fan-out.
    let mut last_read = vec![0; value_types.len()];
    for (index, step) in steps.iter().enumerate() {
        match step {
            PatchStep::Filter { input, .. }
            | PatchStep::Fixture { input, .. }
            | PatchStep::Sink { input, .. } => last_read[*input as usize] = index,
            PatchStep::Source { .. } => {}
        }
    }
    let mut slots = Vec::new();
    let mut available_after = Vec::new();
    let mut remap = vec![0; value_types.len()];
    for (index, step) in steps.iter_mut().enumerate() {
        match step {
            PatchStep::Filter { input, .. }
            | PatchStep::Fixture { input, .. }
            | PatchStep::Sink { input, .. } => *input = remap[*input as usize],
            PatchStep::Source { .. } => {}
        }
        let output = match step {
            PatchStep::Source { output, .. } => output,
            PatchStep::Filter { output_start, .. } | PatchStep::Fixture { output_start, .. } => {
                output_start
            }
            PatchStep::Sink { .. } => continue,
        };
        let value_type = &value_types[*output as usize];
        let slot = slots
            .iter()
            .zip(&available_after)
            .position(|(kind, last)| kind == value_type && *last < index)
            .unwrap_or_else(|| {
                slots.push(value_type.clone());
                available_after.push(0);
                slots.len() - 1
            });
        available_after[slot] = last_read[*output as usize].max(index);
        remap[*output as usize] = index32(slot)?;
        *output = index32(slot)?;
    }
    Ok(PreparedPatch {
        steps: steps.into_boxed_slice(),
        fixture_programs: fixture_programs.into_boxed_slice(),
        value_layouts: slots
            .iter()
            .map(|value_type| value_type.layout(profiles))
            .collect::<Result<_, _>>()
            .map_err(|error| SequenceOutputPrepareError::InvalidPatch(format!("{error:?}")))?,
    })
}

fn fuse_rgb_packing(
    steps: &mut Vec<PatchStep>,
    value_count: usize,
) -> Result<(), SequenceOutputPrepareError> {
    let mut producers = vec![None; value_count];
    let mut readers = vec![0; value_count];
    for (index, step) in steps.iter().enumerate() {
        match step {
            PatchStep::Source { output, .. } => producers[*output as usize] = Some(index),
            PatchStep::Filter {
                input,
                output_start,
                ..
            }
            | PatchStep::Fixture {
                input,
                output_start,
                ..
            } => {
                producers[*output_start as usize] = Some(index);
                readers[*input as usize] += 1;
            }
            PatchStep::Sink { input, .. } => readers[*input as usize] += 1,
        }
    }
    let mut removed = vec![false; steps.len()];
    for index in 0..steps.len() {
        let PatchStep::Filter {
            input,
            output_start,
            filter: PreparedFilter::Quantize8 { width },
        } = &steps[index]
        else {
            continue;
        };
        let (mut cursor, output, width) = (*input, *output_start, *width);
        let mut order = [0u8, 1, 2];
        let mut chain = Vec::new();
        let mut packed = None;
        while readers[cursor as usize] == 1 {
            let Some(producer) = producers[cursor as usize] else {
                break;
            };
            match &steps[producer] {
                PatchStep::Filter {
                    input,
                    filter:
                        PreparedFilter::DimmingCurve { width: count, .. }
                        | PreparedFilter::ScaleInvert { width: count, .. },
                    ..
                } if *count == width => {
                    chain.push(producer);
                    cursor = *input;
                }
                PatchStep::Filter {
                    input,
                    filter:
                        PreparedFilter::ComponentReorder {
                            components_per_cell: 3,
                            order: upstream,
                            ..
                        },
                    ..
                } => {
                    order = order.map(|channel| upstream[usize::from(channel)] as u8);
                    chain.push(producer);
                    cursor = *input;
                }
                PatchStep::Filter {
                    input,
                    filter:
                        PreparedFilter::ColorBreakdown {
                            capability: ColorEncoding::Rgb,
                            cell_count,
                        },
                    ..
                } if cell_count.checked_mul(3) == Some(width) => {
                    chain.push(producer);
                    packed = Some((*input, *cell_count));
                    break;
                }
                _ => break,
            }
        }
        if let Some((input, cell_count)) = packed {
            // Every channel starts as one of 256 RGB8 values. Evaluate fixed
            // component-wise transforms in their original order, using the same
            // filter implementation as unfused playback. Reorders commute with
            // these channel-independent transforms and are composed above.
            let mut values =
                PatchValue::Components((0..=255).map(|value| value as f32 / 255.0).collect());
            let mut temporary = [PatchValue::Components(Vec::with_capacity(256))];
            for &producer in chain.iter().rev() {
                let PatchStep::Filter { filter, .. } = &steps[producer] else {
                    unreachable!()
                };
                let mut filter = filter.clone();
                match &mut filter {
                    PreparedFilter::DimmingCurve { width, .. }
                    | PreparedFilter::ScaleInvert { width, .. } => *width = 256,
                    _ => continue,
                }
                filter.evaluate(&values, &mut temporary).map_err(|error| {
                    SequenceOutputPrepareError::InvalidPatch(format!("{error:?}"))
                })?;
                std::mem::swap(&mut values, &mut temporary[0]);
            }
            let PatchValue::Components(values) = values else {
                unreachable!()
            };
            let lookup = Box::new(std::array::from_fn(|index| {
                dawn_runtime::fixture::quantize8(values[index])
            }));
            let lookup = (!lookup.iter().copied().eq(0..=255)).then_some(lookup);
            steps[index] = PatchStep::Filter {
                input,
                output_start: output,
                filter: PreparedFilter::PackRgb {
                    cell_count,
                    order,
                    lookup,
                },
            };
            for producer in chain {
                removed[producer] = true;
            }
        }
    }
    let mut index = 0;
    steps.retain(|_| {
        let keep = !removed[index];
        index += 1;
        keep
    });
    Ok(())
}
