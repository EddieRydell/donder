use std::collections::BTreeSet;

use donder_runtime::dsl::bytecode::{Instruction, SignalPixel};
use donder_runtime::patch::PreparedPatch;
use donder_runtime::signal::{
    PreparedEffectImplementation, PreparedOperator, PreparedSignalGraph, PreparedSignalKind,
    PreparedTarget,
};

use crate::sequence::composition::graph::finish_signal_plan;

/// The patch has already been lowered for the selected ports. Compact its source
/// cells and their signal dependencies before any runtime workspace is created.
pub(crate) fn compact(signal: &mut PreparedSignalGraph, patch: &mut PreparedPatch) {
    let mut cells = vec![BTreeSet::new(); signal.fixtures.len()];
    for route in &patch.routes {
        for (index, (fixture, &offset)) in signal
            .fixtures
            .iter()
            .zip(&signal.fixture_pixel_offsets)
            .enumerate()
        {
            let end = offset + fixture.pixel_count;
            let start = route.pixels.start.max(offset);
            let end = route.pixels.end.min(end);
            if start < end {
                cells[index].extend((start - offset..end - offset).map(|cell| cell as u32));
            }
        }
    }
    // Spatial queries depend on pixels that need not be patched to this device.
    // Keep the complete coordinate domain whenever a reachable operator can
    // address it; packing remains restricted to the selected output ports.
    let mut reachable = vec![false; signal.plan.nodes.len()];
    reachable[signal.plan.output_index] = true;
    let mut local = false;
    let mut global = false;
    for index in (0..reachable.len()).rev() {
        if !reachable[index] {
            continue;
        }
        match &signal.plan.nodes[index].kind {
            PreparedSignalKind::Layer { .. } => {}
            PreparedSignalKind::Operator {
                operator, inputs, ..
            } => {
                for &input in inputs {
                    reachable[input] = true;
                }
                let PreparedOperator::Dsl(program) = operator.implementation;
                for instruction in &signal.programs[program as usize].instructions {
                    if let Instruction::SignalSample { pixel, .. } = instruction {
                        local |= matches!(pixel, SignalPixel::Local(_));
                        global |= matches!(pixel, SignalPixel::Global(_));
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
        for (index, fixture) in signal.fixtures.iter().enumerate() {
            let cells = &mut cells[index];
            if global || !cells.is_empty() {
                cells.extend(0..fixture.pixel_count as u32);
            }
        }
    }
    let cells = cells
        .into_iter()
        .map(|cells| cells.into_iter().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut retained_global = Vec::new();
    for (index, selected) in cells.iter().enumerate() {
        let offset = signal.fixture_pixel_offsets[index];
        retained_global.extend(selected.iter().map(|cell| offset + *cell as usize));
    }
    for route in &mut patch.routes {
        // Every routed cell was retained above; its insertion point is its new
        // storage address. This never changes the effect's sampling coordinates.
        let start = retained_global.partition_point(|&cell| cell < route.pixels.start);
        let count = route.pixels.end - route.pixels.start;
        route.pixels = start..start + count;
    }
    let mut signal_fixture_map = vec![None; signal.fixtures.len()];
    let mut signal_fixtures = Vec::new();
    let mut offsets = Vec::new();
    let mut pixel_count = 0;
    for (index, fixture) in signal.fixtures.iter().enumerate() {
        if cells[index].is_empty() {
            continue;
        }
        signal_fixture_map[index] = Some(signal_fixtures.len());
        let mut fixture = *fixture;
        fixture.pixel_count = cells[index].len();
        offsets.push(pixel_count);
        pixel_count += fixture.pixel_count;
        signal_fixtures.push(fixture);
    }

    let mut target_map = vec![None; signal.targets.len()];
    let mut targets = Vec::new();
    let mut pixels = Vec::new();
    let mut spatial_contexts = Vec::new();
    // Compaction only removes records; native indices need no narrowing checks.
    let mut retain_target = |old: usize| -> usize {
        if let Some(mapped) = target_map[old as usize] {
            return mapped;
        }
        let mapped = targets.len();
        let start = pixels.len();
        let mut max_count = 0;
        for (local_index, pixel) in signal.target(old).iter().enumerate() {
            let old_fixture = pixel.fixture_index as usize;
            let Some(fixture_index) = signal_fixture_map[old_fixture] else {
                continue;
            };
            let Ok(cell) = cells[old_fixture].binary_search(&pixel.fixture_pixel_index) else {
                continue;
            };
            let mut pixel = pixel.clone();
            // Only storage addresses change. Effect/operator indices, counts and
            // fractions keep the original global or target-local sampling context.
            pixel.fixture_index = fixture_index;
            pixel.fixture_pixel_index = cell as u32;
            max_count = max_count.max(pixel.pixel_count as usize);
            pixels.push(pixel);
            if !signal.spatial_contexts.is_empty() {
                spatial_contexts.push(
                    signal.spatial_contexts
                        [signal.targets[old as usize].pixels.start as usize + local_index],
                );
            }
        }
        let end = pixels.len();
        targets.push(PreparedTarget {
            pixels: start..end,
            sample_count: if end - start > max_count {
                max_count
            } else {
                0
            },
        });
        target_map[old as usize] = Some(mapped);
        mapped
    };
    let target = retain_target(signal.plan.target);

    // The dependency walk above already includes every input, including temporal
    // samples. An empty pixel selection needs only an empty output node.
    if pixel_count == 0 {
        reachable.fill(false);
        reachable[signal.plan.output_index] = true;
    }
    let mut nodes = Vec::new();
    let mut node_map = vec![0; reachable.len()];
    let mut layers = Vec::new();
    let mut effects_by_layer = Vec::new();
    let mut effects = Vec::new();
    let mut effect_map = vec![None; signal.effects.len()];
    let mut effect_automation_count = 0;
    let mut operator_automation_count = 0;
    for (index, node) in signal.plan.nodes.iter().enumerate() {
        if !reachable[index] {
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
                                && cells[old_fixture]
                                    .binary_search(&pixel.fixture_pixel_index)
                                    .is_ok()
                        });
                        if !intersects {
                            continue;
                        }
                        let mut effect = effect.clone();
                        effect.target = retain_target(effect.target);
                        if let Some(automation) = &mut effect.automation {
                            automation.workspace_slot = effect_automation_count;
                            effect_automation_count += 1;
                        }
                        retained.push(effects.len());
                        effect_map[effect_index] = Some(effects.len());
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
                operator_automation_count += usize::from(!automation.is_empty());
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
    let clips = signal
        .clips
        .iter()
        .map(|clip| {
            let mut clip = clip.clone();
            clip.target = retain_target(clip.target);
            clip.effects = clip
                .effects
                .iter()
                .filter_map(|&effect| effect_map[effect])
                .collect();
            clip
        })
        .collect();
    let mut programs = Vec::new();
    let mut program_map = vec![None; signal.programs.len()];
    let mut retain_program = |program: &mut usize| {
        let old = *program as usize;
        *program = if let Some(mapped) = program_map[old] {
            mapped
        } else {
            let mapped = programs.len();
            programs.push(signal.programs[old].clone());
            program_map[old] = Some(mapped);
            mapped
        };
    };
    for effect in &mut effects {
        let program = match &mut effect.implementation {
            PreparedEffectImplementation::Dsl { program, .. }
            | PreparedEffectImplementation::Bound { program, .. } => program,
        };
        retain_program(program);
    }
    for node in &mut nodes {
        if let PreparedSignalKind::Operator { operator, .. } = &mut node.kind {
            let PreparedOperator::Dsl(program) = &mut operator.implementation;
            retain_program(program);
        }
    }
    signal.plan = finish_signal_plan(nodes, node_map[signal.plan.output_index], target);
    signal.fixtures = signal_fixtures.into_boxed_slice();
    signal.fixture_pixel_offsets = offsets.into_boxed_slice();
    signal.pixel_count = pixel_count;
    crate::sequence::effects::retained::compact_environments(
        &mut signal.parameter_environments,
        &mut effects,
    );
    signal.effects = effects.into_boxed_slice();
    signal.clips = clips;
    signal.effects_by_layer = effects_by_layer.into_boxed_slice();
    signal.layers = layers.into_boxed_slice();
    signal.programs = programs.into_boxed_slice();
    signal.targets = targets.into_boxed_slice();
    signal.target_pixels = pixels.into_boxed_slice();
    signal.spatial_contexts = spatial_contexts.into_boxed_slice();
}
