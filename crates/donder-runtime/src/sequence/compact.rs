//! Selected-output storage compaction over admitted programs and binding plans.
//! Sampling coordinates survive; only physical storage addresses are remapped.
use super::programs::{AdmittedPrograms, ExecutableGraph};
use crate::bindings::ExecutableEnvironment;
use crate::dsl::AutomationPlan;
use crate::dsl::bytecode::{Instruction, SignalPixel};
use crate::patch::PreparedPatch;
use crate::signal::{
    PreparedEffectImplementation, PreparedOperator, PreparedSignalKind, PreparedSignalNode,
    PreparedTarget, SignalPlan,
};
use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet},
    vec,
    vec::Vec,
};

/// The builder has already emitted only the selected ports and their routes.
pub(super) fn compact(signal: &mut ExecutableGraph, patch: &mut PreparedPatch) {
    let mut cells = vec![BTreeSet::new(); signal.fixtures.len()];
    for route in &patch.routes {
        for (index, (fixture, &offset)) in signal
            .fixtures
            .iter()
            .zip(&signal.fixture_pixel_offsets)
            .enumerate()
        {
            let start = route.pixels.start.max(offset);
            let end = route.pixels.end.min(offset + fixture.pixel_count);
            if start < end {
                cells[index].extend((start - offset..end - offset).map(|cell| cell as u32));
            }
        }
    }
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
                for instruction in &signal.programs.operator(program).bytecode().instructions {
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
    // Local queries need every original cell of each selected fixture. Global
    // queries can address unpatched fixtures and therefore retain the full rig.
    if cells.iter().any(|cells| !cells.is_empty()) && (local || global) {
        for (fixture, cells) in signal.fixtures.iter().zip(&mut cells) {
            if global || !cells.is_empty() {
                cells.extend(0..fixture.pixel_count as u32);
            }
        }
    }
    let cells: Vec<Vec<_>> = cells
        .into_iter()
        .map(|cells| cells.into_iter().collect())
        .collect();
    let retained_global: Vec<_> = cells
        .iter()
        .zip(&signal.fixture_pixel_offsets)
        .flat_map(|(selected, &offset)| selected.iter().map(move |cell| offset + *cell as usize))
        .collect();
    for route in &mut patch.routes {
        let start = retained_global.partition_point(|&cell| cell < route.pixels.start);
        route.pixels = start..start + route.pixels.len();
    }
    let mut fixture_map = vec![None; signal.fixtures.len()];
    let mut fixtures = Vec::new();
    let mut offsets = Vec::new();
    let mut pixel_count = 0;
    for (index, fixture) in signal.fixtures.iter().enumerate() {
        if cells[index].is_empty() {
            continue;
        }
        fixture_map[index] = Some(fixtures.len());
        let mut fixture = *fixture;
        fixture.pixel_count = cells[index].len();
        offsets.push(pixel_count);
        pixel_count += fixture.pixel_count;
        fixtures.push(fixture);
    }
    let mut target_map = vec![None; signal.targets.len()];
    let mut targets = Vec::new();
    let mut pixels = Vec::new();
    let mut spatial = Vec::new();
    let mut retain_target = |old: usize| {
        if let Some(index) = target_map[old] {
            return index;
        }
        let index = targets.len();
        let start = pixels.len();
        let mut max_count = 0;
        for (local_index, pixel) in signal.target(old).iter().enumerate() {
            let old_fixture = pixel.fixture_index;
            let Some(fixture_index) = fixture_map[old_fixture] else {
                continue;
            };
            let Ok(cell) = cells[old_fixture].binary_search(&pixel.fixture_pixel_index) else {
                continue;
            };
            let mut pixel = *pixel;
            pixel.fixture_index = fixture_index;
            pixel.fixture_pixel_index = cell as u32;
            max_count = max_count.max(pixel.pixel_count);
            pixels.push(pixel);
            if !signal.spatial_contexts.is_empty() {
                spatial
                    .push(signal.spatial_contexts[signal.targets[old].pixels.start + local_index]);
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
        target_map[old] = Some(index);
        index
    };
    let target = retain_target(signal.plan.target);
    if pixel_count == 0 {
        reachable.fill(false);
        reachable[signal.plan.output_index] = true;
    }
    let mut node_map = vec![0; reachable.len()];
    let mut nodes = Vec::new();
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
                        if !signal.target(effect.target).iter().any(|pixel| {
                            fixture_map[pixel.fixture_index].is_some()
                                && cells[pixel.fixture_index]
                                    .binary_search(&pixel.fixture_pixel_index)
                                    .is_ok()
                        }) {
                            continue;
                        }
                        // An effect may be referenced by multiple layer nodes.
                        let mapped = match effect_map[effect_index] {
                            Some(mapped) => mapped,
                            None => {
                                let mut effect = effect.clone();
                                effect.target = retain_target(effect.target);
                                if let Some(automation) = &mut effect.automation {
                                    automation.workspace_slot = effect_automation_count;
                                    effect_automation_count += 1;
                                }
                                let mapped = effects.len();
                                effect_map[effect_index] = Some(mapped);
                                effects.push(effect);
                                mapped
                            }
                        };
                        retained.push(mapped);
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
    let mut samples = Vec::new();
    let mut sample_map = BTreeMap::new();
    for effect in &mut effects {
        let program = match &mut effect.implementation {
            PreparedEffectImplementation::Dsl { program, .. }
            | PreparedEffectImplementation::Bound { program, .. } => program,
        };
        *program = *sample_map.entry(*program).or_insert_with(|| {
            let index = samples.len();
            samples.push(signal.programs.sample(*program).clone());
            index
        });
    }
    let mut operators = Vec::new();
    let mut operator_map = BTreeMap::new();
    for node in &mut nodes {
        if let PreparedSignalKind::Operator { operator, .. } = &mut node.kind {
            let PreparedOperator::Dsl(program) = &mut operator.implementation;
            *program = *operator_map.entry(*program).or_insert_with(|| {
                let index = operators.len();
                operators.push(signal.programs.operator(*program).clone());
                index
            });
        }
    }
    retain_environments(&mut signal.parameter_environments, &mut effects);
    signal.plan = finish_plan(nodes, node_map[signal.plan.output_index], target);
    signal.fixtures = fixtures.into();
    signal.fixture_pixel_offsets = offsets.into();
    signal.pixel_count = pixel_count;
    signal.effects = effects.into();
    signal.clips = clips;
    signal.effects_by_layer = effects_by_layer.into();
    signal.layers = layers.into();
    signal.programs = AdmittedPrograms::new(samples.into(), operators.into());
    signal.targets = targets.into();
    signal.target_pixels = pixels.into();
    signal.spatial_contexts = spatial.into();
}

/// Retain only transitive dependencies of live sample environments. This is
/// needed for full builds too: constant specializations leave no retained work.
pub(super) fn retain_environments(
    environments: &mut Box<[ExecutableEnvironment]>,
    effects: &mut [crate::signal::PreparedEffect<AutomationPlan>],
) {
    let mut required = vec![false; environments.len()];
    for effect in effects.iter() {
        if let PreparedEffectImplementation::Bound { environment, .. } = effect.implementation {
            required[environment] = true;
        }
    }
    for index in (0..required.len()).rev() {
        if required[index] {
            for binding in &environments[index].bindings {
                required[binding.source_environment()] = true;
            }
        }
    }
    let mut environment_map = vec![0; required.len()];
    let mut retained = Vec::new();
    for (index, mut environment) in core::mem::take(environments)
        .into_vec()
        .into_iter()
        .enumerate()
    {
        if !required[index] {
            continue;
        }
        environment_map[index] = retained.len();
        for binding in &mut environment.bindings {
            binding.remap_environment(environment_map[binding.source_environment()]);
        }
        retained.push(environment);
    }
    for effect in effects {
        if let PreparedEffectImplementation::Bound { environment, .. } = &mut effect.implementation
        {
            *environment = environment_map[*environment];
        }
    }
    *environments = retained.into();
}

pub(super) fn finish_plan(
    mut nodes: Vec<PreparedSignalNode<AutomationPlan>>,
    output_index: usize,
    target: usize,
) -> SignalPlan<AutomationPlan> {
    let mut depths = Vec::with_capacity(nodes.len());
    let mut vm_workspace_count = 0;
    for node in &mut nodes {
        let depth = match &mut node.kind {
            PreparedSignalKind::Layer { .. } => 0,
            PreparedSignalKind::Output { inputs } => inputs
                .iter()
                .fold(0, |depth, &input| depth.max(depths[input])),
            PreparedSignalKind::Operator {
                inputs, vm_slot, ..
            } => {
                *vm_slot = inputs
                    .iter()
                    .fold(0, |depth, &input| depth.max(depths[input]));
                *vm_slot + 1
            }
        };
        vm_workspace_count = vm_workspace_count.max(depth);
        depths.push(depth);
    }
    let mut required = vec![false; nodes.len()];
    required[output_index] = true;
    for index in (0..nodes.len()).rev() {
        if required[index] {
            for &input in frame_inputs(&nodes[index]) {
                required[input] = true;
            }
        }
    }
    let mut consumers = vec![0usize; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        if required[index] {
            for &input in frame_inputs(node) {
                consumers[input] += 1;
            }
        }
    }
    let mut frame_nodes = Vec::new();
    let mut frame_slots = vec![u32::MAX as usize; nodes.len()];
    let mut available = Vec::new();
    let mut frame_buffer_count = 0;
    for (index, node) in nodes.iter().enumerate() {
        if !required[index] {
            continue;
        }
        if index == output_index
            && let PreparedSignalKind::Output { inputs } = &node.kind
            && let [input] = inputs.as_ref()
        {
            frame_slots[index] = frame_slots[*input];
            continue;
        }
        frame_slots[index] = match available.pop() {
            Some(slot) => slot,
            None => {
                let slot = frame_buffer_count;
                frame_buffer_count += 1;
                slot
            }
        };
        frame_nodes.push(index);
        for &input in frame_inputs(node) {
            consumers[input] -= 1;
            if consumers[input] == 0 {
                available.push(frame_slots[input]);
            }
        }
    }
    SignalPlan {
        output_index,
        target,
        nodes: nodes.into(),
        vm_workspace_count,
        frame_nodes: frame_nodes.into(),
        frame_slots: frame_slots.into(),
        frame_buffer_count,
    }
}

fn frame_inputs(node: &PreparedSignalNode<AutomationPlan>) -> &[usize] {
    match &node.kind {
        PreparedSignalKind::Output { inputs } => inputs,
        PreparedSignalKind::Layer { .. } | PreparedSignalKind::Operator { .. } => &[],
    }
}

#[cfg(test)]
mod tests {
    mod storage;

    use super::*;
    use crate::dsl::bytecode::{
        BytecodeProgram, ColorSlot, ContextRead, FloatSlot, IntSlot, NumberSlot, SlotLayout,
    };
    use crate::dsl::{
        CompiledOperator, DslBindCache, Identifier, OperatorInputDecl, SampleProgram,
    };
    use crate::sequence::{
        FixtureGeometry, OperatorDefinition, OutputEncoding, PreparedSequence, RgbOrder,
        SampleDefinition, SequenceTiming, SequenceWindow, TargetScope,
    };
    use crate::values::SampleTime;
    use core::num::NonZeroU32;

    #[test]
    fn terminal_output_aliases_one_input_but_composes_multiple_inputs() {
        let mut nodes = vec![
            PreparedSignalNode {
                kind: PreparedSignalKind::Layer { layer_index: 0 },
            },
            PreparedSignalNode {
                kind: PreparedSignalKind::Output {
                    inputs: vec![0].into(),
                },
            },
        ];
        let plan = finish_plan(nodes.clone(), 1, 0);
        assert_eq!(plan.frame_nodes.as_ref(), [0]);
        assert_eq!(plan.frame_slots.as_ref(), [0, 0]);
        assert_eq!(plan.frame_buffer_count, 1);

        nodes[1].kind = PreparedSignalKind::Layer { layer_index: 1 };
        nodes.push(PreparedSignalNode {
            kind: PreparedSignalKind::Output {
                inputs: vec![0, 1].into(),
            },
        });
        let plan = finish_plan(nodes, 2, 0);
        assert_eq!(plan.frame_nodes.as_ref(), [0, 1, 2]);
        assert_eq!(plan.frame_slots.as_ref(), [0, 1, 2]);
        assert_eq!(plan.frame_buffer_count, 3);
    }

    fn program(
        instructions: Vec<Instruction>,
        layout: SlotLayout,
        pixel_entry: u32,
    ) -> BytecodeProgram {
        BytecodeProgram {
            instructions: instructions.into(),
            array_constants: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            curves: Box::new([]),
            targets: Box::new([]),
            target_lists: Box::new([]),
            target_items: Box::new([]),
            gradients: Box::new([]),
            value_operands: Box::new([]),
            array_types: Box::new([]),
            layout,
            uses_pixel_context: true,
            pixel_entry,
            array_capacity: 0,
            array_width: 0,
            loop_count: 0,
        }
    }

    fn sequence(compacted: bool, routed: bool, query: SignalPixel<i32>) -> PreparedSequence {
        let sample = SampleProgram::admit(
            program(
                vec![
                    Instruction::ContextRead {
                        dst: NumberSlot::Float(FloatSlot(0)),
                        read: ContextRead::Seconds,
                    },
                    Instruction::ContextRead {
                        dst: NumberSlot::Float(FloatSlot(1)),
                        read: ContextRead::PixelFraction,
                    },
                    Instruction::ContextRead {
                        dst: NumberSlot::Float(FloatSlot(2)),
                        read: ContextRead::PixelX,
                    },
                    Instruction::Rgb {
                        dst: ColorSlot(0),
                        red: FloatSlot(1),
                        green: FloatSlot(2),
                        blue: FloatSlot(0),
                    },
                    Instruction::ReturnColor(ColorSlot(0)),
                ],
                SlotLayout {
                    floats: 3,
                    colors: 1,
                    ..SlotLayout::default()
                },
                1,
            ),
            Box::new([]),
        )
        .unwrap();
        let query_index = match query {
            SignalPixel::Current => 0,
            SignalPixel::Local(index) | SignalPixel::Global(index) => index,
        };
        let operator = CompiledOperator::admit(
            Identifier::new("query".into()).unwrap(),
            vec![OperatorInputDecl {
                name: Identifier::new("source".into()).unwrap(),
            }],
            vec![],
            program(
                vec![
                    Instruction::LoadFloatConst {
                        dst: FloatSlot(0),
                        bits: 0.3f32.to_bits(),
                    },
                    Instruction::LoadIntConst {
                        dst: IntSlot(0),
                        value: query_index,
                    },
                    Instruction::SignalSample {
                        capability: (),
                        dst: ColorSlot(0),
                        input: 0,
                        seconds: FloatSlot(0),
                        pixel: query.map(|_| IntSlot(0)),
                        frame_cache: 0,
                    },
                    Instruction::ReturnColor(ColorSlot(0)),
                ],
                SlotLayout {
                    floats: 1,
                    ints: 1,
                    colors: 1,
                    ..SlotLayout::default()
                },
                2,
            ),
        )
        .unwrap();
        let mut cache = DslBindCache::default();
        let sample = SampleDefinition::new(sample)
            .bind(vec![], &mut cache)
            .unwrap();
        let operator = OperatorDefinition::new(operator)
            .bind(vec![], &mut cache)
            .unwrap();
        let timing = SequenceTiming::admit(
            NonZeroU32::new(60).unwrap(),
            NonZeroU32::new(60).unwrap(),
            NonZeroU32::new(1_000_000).unwrap(),
            vec![SequenceWindow {
                start: SampleTime::from_ticks(0),
                duration: NonZeroU32::new(800_000).unwrap(),
            }]
            .into(),
        )
        .unwrap();
        PreparedSequence::build(timing, |builder| {
            let a = builder.fixture(
                10,
                FixtureGeometry::admit((0..8).map(|cell| [cell as f32 / 12.0, 0.0]).collect())
                    .unwrap(),
            );
            let b = builder.fixture(
                20,
                FixtureGeometry::admit((8..12).map(|cell| [cell as f32 / 12.0, 0.0]).collect())
                    .unwrap(),
            );
            let target = builder.target([b, a], TargetScope::WholeTarget);
            let window = builder.windows().next().unwrap();
            let effect = builder.sample(&sample, window, target);
            builder.clip(7, window, target, [effect]);
            builder.layer(false, [effect]);
            let layer = builder.layer(true, [effect]);
            let inner = builder.operator(&operator, |_| layer);
            let outer = builder.operator(&operator, |_| inner);
            if routed {
                let output = builder.port(0, 1);
                builder.padding(output, 2);
                for range in [2..3, 6..7] {
                    let span = builder.target_slice(target, range);
                    builder.route(output, span, OutputEncoding::Rgb(RgbOrder::Rgb), None);
                }
                builder.padding(output, 1);
            }
            if compacted {
                builder.compact_to_outputs();
            }
            builder.output([outer])
        })
    }

    #[test]
    fn selected_spans_match_full_playback_across_query_domains_and_seeks() {
        for (query, count) in [
            (SignalPixel::Current, 2),
            (SignalPixel::Local(6), 8),
            (SignalPixel::Global(9), 12),
        ] {
            let full = sequence(false, true, query);
            let compacted = sequence(true, true, query);
            assert_eq!(full.pixel_count(), 12);
            assert_eq!(compacted.pixel_count(), count);
            let compact_raw = compacted.archive_data().signals;
            let full_raw = full.archive_data().signals;
            // Operators request their inputs at the query time; neither plan
            // eagerly renders upstream layers, and the terminal output aliases.
            assert_eq!(full_raw.plan.frame_nodes.as_ref(), [3]);
            assert_eq!(compact_raw.plan.frame_nodes.as_ref(), [2]);
            assert_eq!(full_raw.plan.frame_buffer_count, 1);
            assert_eq!(compact_raw.plan.frame_buffer_count, 1);
            let retained = compact_raw.target(compact_raw.effects[0].target);
            let original = full_raw.target(full_raw.effects[0].target);
            let original_index = if count == 2 { 2 } else { 0 };
            assert_eq!(
                retained[0].pixel_index,
                original[original_index].pixel_index
            );
            assert_eq!(
                retained[0].pixel_fraction,
                original[original_index].pixel_fraction
            );
            assert_eq!(
                compact_raw.spatial_contexts[compact_raw.targets[compact_raw.effects[0].target]
                    .pixels
                    .start],
                full_raw.spatial_contexts
                    [full_raw.targets[full_raw.effects[0].target].pixels.start + original_index]
            );
            let bytes = crate::wire::encode_sequence(&compacted).unwrap();
            let decoded =
                crate::wire::decode_sequence(&bytes, crate::wire::LoadLimits::default()).unwrap();
            let mut full = full.into_playback();
            let mut compacted = compacted.into_playback();
            let mut decoded = decoded.into_playback();
            for ticks in [900_000, 0, 500_000, 250_000, 1_000_000] {
                let time = SampleTime::from_ticks(ticks);
                let expected = full.evaluate(time);
                let expected = expected.outputs().next().unwrap().bytes;
                assert_eq!(
                    compacted.evaluate(time).outputs().next().unwrap().bytes,
                    expected
                );
                assert_eq!(
                    decoded.evaluate(time).outputs().next().unwrap().bytes,
                    expected
                );
            }
        }
    }

    #[test]
    fn empty_selected_outputs_remove_all_execution_dependencies() {
        let full = sequence(false, false, SignalPixel::Global(9));
        let before = full.archive_data();
        assert!(!before.signals.programs.is_empty());
        assert!(!before.signals.effects.is_empty());
        assert!(!before.signals.target_pixels.is_empty());
        let compacted = sequence(true, false, SignalPixel::Global(9));
        assert_eq!(compacted.pixel_count(), 0);
        let data = compacted.archive_data();
        assert!(data.outputs.is_empty());
        assert!(data.patch.routes.is_empty());
        let raw = data.signals;
        assert!(raw.fixtures.is_empty());
        assert!(raw.fixture_pixel_offsets.is_empty());
        assert!(raw.programs.is_empty());
        assert!(raw.effects.is_empty());
        assert!(raw.parameter_environments.is_empty());
        assert!(raw.target_pixels.is_empty());
        assert_eq!(raw.plan.nodes.len(), 1);
        assert!(
            compacted
                .into_playback()
                .evaluate(SampleTime::from_ticks(0))
                .colors()
                .is_empty()
        );
    }
}
