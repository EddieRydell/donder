use std::collections::{BTreeSet, HashMap};

use dawn_runtime::dsl::bytecode::{Instruction, SignalPixel};
use dawn_runtime::sequence::PreparedSequence;
use dawn_runtime::signal::{
    BoundEffectImplementation, PreparedEffectImplementation, PreparedOperator, PreparedSignalKind,
    PreparedTarget,
};

use crate::RenderError;
use crate::sequence::composition::graph::finish_signal_plan;

/// The patch has already been lowered for the selected ports. Compact its source
/// cells and their signal dependencies before any runtime workspace is created.
pub(super) fn compact(sequence: &mut PreparedSequence) -> Result<(), RenderError> {
    let signal = &sequence.signals;
    let mut cells = vec![BTreeSet::new(); signal.fixtures.len()];
    for route in &sequence.patch.routes {
        for (index, (fixture, &offset)) in signal
            .fixtures
            .iter()
            .zip(&signal.fixture_pixel_offsets)
            .enumerate()
        {
            let offset = index32(offset)?;
            let end = offset
                .checked_add(index32(fixture.pixel_count)?)
                .ok_or(RenderError::BadTarget)?;
            let start = route.pixels.start.max(offset);
            let end = route.pixels.end.min(end);
            if start < end {
                cells[index].extend(start - offset..end - offset);
            }
        }
    }
    // Spatial queries depend on pixels that need not be patched to this device.
    // Keep the complete coordinate domain whenever a reachable operator can
    // address it; packing remains restricted to the selected output ports.
    let mut reachable = vec![false; sequence.signals.plan.nodes.len()];
    reachable[sequence.signals.plan.output_index] = true;
    let mut local = false;
    let mut global = false;
    for index in (0..reachable.len()).rev() {
        if !reachable[index] {
            continue;
        }
        match &sequence.signals.plan.nodes[index].kind {
            PreparedSignalKind::Layer { .. } => {}
            PreparedSignalKind::Operator {
                operator, inputs, ..
            } => {
                for &input in inputs {
                    reachable[input] = true;
                }
                if let PreparedOperator::Dsl(program) = operator.implementation {
                    for instruction in &sequence.signals.programs[program as usize].instructions {
                        if let Instruction::SignalSample { pixel, .. } = instruction {
                            local |= matches!(pixel, SignalPixel::Local(_));
                            global |= matches!(pixel, SignalPixel::Global(_));
                        }
                    }
                }
            }
            PreparedSignalKind::Output { inputs } => {
                for &input in inputs {
                    reachable[input] = true;
                }
            }
        }
    }
    if cells.iter().any(|cells| !cells.is_empty()) && (local || global) {
        for (index, fixture) in sequence.signals.fixtures.iter().enumerate() {
            let cells = &mut cells[index];
            if global || !cells.is_empty() {
                cells.extend(0..index32(fixture.pixel_count)?);
            }
        }
    }
    let cells = cells
        .into_iter()
        .map(|cells| cells.into_iter().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let signal = &mut sequence.signals;
    let mut retained_global = Vec::new();
    for (index, selected) in cells.iter().enumerate() {
        let offset = index32(signal.fixture_pixel_offsets[index])?;
        retained_global.extend(selected.iter().map(|cell| offset + cell));
    }
    for route in &mut sequence.patch.routes {
        let start = retained_global
            .binary_search(&route.pixels.start)
            .map_err(|_| RenderError::BadTarget)?;
        let count = route.pixels.end - route.pixels.start;
        route.pixels = index32(start)?
            ..index32(start)?
                .checked_add(count)
                .ok_or(RenderError::BadTarget)?;
    }
    let outer_indices = signal
        .fixtures
        .iter()
        .enumerate()
        .map(|(index, fixture)| (fixture.id, index))
        .collect::<HashMap<_, _>>();
    let mut signal_fixture_map = vec![None; signal.fixtures.len()];
    let mut signal_fixtures = Vec::new();
    let mut offsets = Vec::new();
    let mut pixel_count = 0;
    for (index, fixture) in signal.fixtures.iter().enumerate() {
        if cells[index].is_empty() {
            continue;
        }
        signal_fixture_map[index] =
            Some(u16::try_from(signal_fixtures.len()).map_err(|_| RenderError::BadTarget)?);
        let mut fixture = *fixture;
        fixture.pixel_count = cells[index].len();
        offsets.push(pixel_count);
        pixel_count += fixture.pixel_count;
        signal_fixtures.push(fixture);
    }

    let mut target_map = vec![None; signal.targets.len()];
    let mut targets = Vec::new();
    let mut pixels = Vec::new();
    let mut retain_target = |old: u32| -> Result<u32, RenderError> {
        if let Some(mapped) = target_map[old as usize] {
            return Ok(mapped);
        }
        let mapped = index32(targets.len())?;
        let start = index32(pixels.len())?;
        let mut max_count = 0;
        for pixel in signal.target(old) {
            let old_fixture = pixel.fixture_index as usize;
            let Some(fixture_index) = signal_fixture_map[old_fixture] else {
                continue;
            };
            let outer = outer_indices[&signal.fixtures[old_fixture].id];
            let Ok(cell) = cells[outer].binary_search(&u32::from(pixel.fixture_pixel_index)) else {
                continue;
            };
            let mut pixel = pixel.clone();
            // Only storage addresses change. Effect/operator indices, counts and
            // fractions keep the original global or target-local sampling context.
            pixel.fixture_index = fixture_index;
            pixel.fixture_pixel_index = u16::try_from(cell).map_err(|_| RenderError::BadTarget)?;
            max_count = max_count.max(pixel.pixel_count);
            pixels.push(pixel);
        }
        let end = index32(pixels.len())?;
        targets.push(PreparedTarget {
            pixels: start..end,
            sample_count: if end - start > max_count {
                max_count
            } else {
                0
            },
        });
        target_map[old as usize] = Some(mapped);
        Ok(mapped)
    };
    let target = retain_target(signal.plan.target)?;

    let mut required = vec![false; signal.plan.nodes.len()];
    required[signal.plan.output_index] = true;
    // Prepared nodes are topologically ordered. Walk all input dependencies,
    // including temporal samples, without interpreting operator behavior.
    for index in (0..required.len()).rev() {
        if !required[index] || pixel_count == 0 {
            continue;
        }
        match &signal.plan.nodes[index].kind {
            PreparedSignalKind::Layer { .. } => {}
            PreparedSignalKind::Operator { inputs, .. } | PreparedSignalKind::Output { inputs } => {
                for &input in inputs {
                    required[input] = true;
                }
            }
        }
    }
    let mut nodes = Vec::new();
    let mut node_map = vec![0; required.len()];
    let mut layers = Vec::new();
    let mut effects_by_layer = Vec::new();
    let mut effects = Vec::new();
    let mut effect_automation_count = 0;
    let mut operator_automation_count = 0;
    for (index, node) in signal.plan.nodes.iter().enumerate() {
        if !required[index] {
            continue;
        }
        let mut node = node.clone();
        match &mut node.kind {
            PreparedSignalKind::Layer { layer_index } => {
                let mut retained = Vec::new();
                if signal.layers[*layer_index].enabled {
                    for &effect_index in &signal.effects_by_layer[*layer_index] {
                        let effect = &signal.effects[effect_index];
                        // Check intersection before interning a target or cloning its resources.
                        let intersects = signal.target(effect.target).iter().any(|pixel| {
                            let old_fixture = pixel.fixture_index as usize;
                            signal_fixture_map[old_fixture].is_some()
                                && cells[outer_indices[&signal.fixtures[old_fixture].id]]
                                    .binary_search(&u32::from(pixel.fixture_pixel_index))
                                    .is_ok()
                        });
                        if !intersects {
                            continue;
                        }
                        let mut effect = effect.clone();
                        effect.target = retain_target(effect.target)?;
                        if let Some(automation) = &mut effect.automation {
                            automation.workspace_slot = effect_automation_count;
                            effect_automation_count += 1;
                        }
                        retained.push(effects.len());
                        effects.push(effect);
                    }
                }
                layers.push(signal.layers[*layer_index]);
                *layer_index = effects_by_layer.len();
                effects_by_layer.push(retained.into_boxed_slice());
            }
            PreparedSignalKind::Operator {
                operator,
                inputs,
                automation,
                ..
            } => {
                for input in inputs {
                    *input = node_map[*input];
                }
                operator.automation_slot = operator_automation_count;
                operator_automation_count += u32::from(!automation.is_empty());
            }
            PreparedSignalKind::Output { inputs } => {
                if pixel_count == 0 {
                    *inputs = Box::new([]);
                }
                for input in inputs {
                    *input = node_map[*input];
                }
            }
        }
        node_map[index] = nodes.len();
        nodes.push(node);
    }
    let mut programs = Vec::new();
    let mut program_map = vec![None; signal.programs.len()];
    let mut retain_program = |program: &mut u32| -> Result<(), RenderError> {
        let old = *program as usize;
        *program = if let Some(mapped) = program_map[old] {
            mapped
        } else {
            let mapped = index32(programs.len())?;
            programs.push(signal.programs[old].clone());
            program_map[old] = Some(mapped);
            mapped
        };
        Ok(())
    };
    for effect in &mut effects {
        if let PreparedEffectImplementation::Dsl { program, .. }
        | PreparedEffectImplementation::Bound {
            implementation: BoundEffectImplementation::Dsl(program),
            ..
        } = &mut effect.implementation
        {
            retain_program(program)?;
        }
    }
    for node in &mut nodes {
        if let PreparedSignalKind::Operator { operator, .. } = &mut node.kind
            && let PreparedOperator::Dsl(program) = &mut operator.implementation
        {
            retain_program(program)?;
        }
    }
    signal.plan = finish_signal_plan(nodes, node_map[signal.plan.output_index], target)?;
    signal.fixtures = signal_fixtures.into_boxed_slice();
    signal.fixture_pixel_offsets = offsets.into_boxed_slice();
    signal.pixel_count = pixel_count;
    crate::sequence::effects::retained::compact_environments(
        &mut signal.parameter_environments,
        &mut effects,
    )?;
    signal.effects = effects.into_boxed_slice();
    signal.effects_by_layer = effects_by_layer.into_boxed_slice();
    signal.layers = layers.into_boxed_slice();
    signal.programs = programs.into_boxed_slice();
    signal.targets = targets.into_boxed_slice();
    signal.target_pixels = pixels.into_boxed_slice();
    Ok(())
}

fn index32(index: usize) -> Result<u32, RenderError> {
    u32::try_from(index).map_err(|_| RenderError::BadTarget)
}
