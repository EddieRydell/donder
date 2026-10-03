use std::{fmt::Write, fs, path::PathBuf};

use donder_language::dsl::bytecode::Instruction;
use donder_language::dsl::{Type, Value, compile_effects};

#[allow(dead_code)]
#[path = "../../crates/donder-runtime/benches/fixtures/mod.rs"]
mod fixtures;
#[path = "src/mark_workload.rs"]
mod mark_workload;
#[path = "src/workload.rs"]
mod workload;

fn main() {
    println!("cargo:rustc-link-arg=-Tlinkall.x");
    println!(
        "cargo:rustc-link-search={}",
        std::env::var("CARGO_MANIFEST_DIR").unwrap()
    );
    println!("cargo:rerun-if-changed=rwtext_hook.x");
    println!("cargo:rerun-if-changed=../../crates/donder-runtime/benches/fixtures/mod.rs");
    println!("cargo:rerun-if-changed=../../examples/starter/effects");
    println!("cargo:rerun-if-changed=../../examples/starter/operators");
    println!("cargo:rerun-if-changed=src/workload.rs");
    println!("cargo:rerun-if-changed=src/mark_workload.rs");
    println!(
        "cargo:rerun-if-changed=../../crates/donder-runtime/tests/fixtures/array-lifetimes.effect.donder"
    );
    let mut generated = String::from(
        "use alloc::{boxed::Box, vec};\n\
         use donder_language::Shared as Arc;\n\
         use donder_language::dsl::{BoundParams, Identifier, SampleProgram, Type, Value};\n\
         use donder_language::dsl::bytecode::{ArithmeticOp, ArraySlot, BoolSlot, BytecodeProgram, ColorBinary, ColorComponent, ColorSlot, CompareOp, ConstantId, ContextRead, CurveSlot, EnumSlot, EnumSlotType, FloatBinary, FloatSlot, FloatUnary, GradientSlot, Instruction, IntArithmeticOp, IntSlot, LocalId, MarkOp, MarksSlot, NumberSlot, ParamId, ParameterKind, PoolSpan, SignalPixel, SlotLayout, Target, ValueSlot};\n\
         use donder_language::values::{Color, Curve, CurvePoint, Gradient, GradientStop};\n",
    );
    let mut golden = Vec::new();
    let gamma_lookup = workload::gamma_lookup();
    let mut gamma_golden = Vec::new();
    let operator = donder_language::dsl::compile_operators(workload::OPERATOR_SOURCE)
        .unwrap()
        .remove(0);
    let mut operator_golden = Vec::new();
    let mut nested_golden = Vec::new();
    let grouped = donder_language::dsl::compile_operators(workload::GROUPED_SOURCE)
        .unwrap()
        .remove(0);
    let alternating = donder_language::dsl::compile_operators(workload::ALTERNATING_SOURCE)
        .unwrap()
        .remove(0);
    let mut temporal_golden = Vec::new();
    let mut pulse_automation_golden = Vec::new();
    let mut empty_golden = Vec::new();
    let identity = donder_language::dsl::compile_operators(workload::IDENTITY_SOURCE)
        .unwrap()
        .remove(0);
    let mut mixed_golden = Vec::new();
    let invert = donder_language::dsl::compile_operators(include_str!(
        "../../examples/starter/operators/standard.operator.donder"
    ))
    .unwrap()
    .into_iter()
    .find(|operator| operator.name().as_str() == "Invert")
    .unwrap();
    let mut names = Vec::new();
    let pulse_program = compile_effects(include_str!(
        "../../examples/starter/effects/standard.effect.donder"
    ))
    .unwrap()
    .remove(0)
    .sample_program()
    .clone();
    let pulse_params = indexmap::IndexMap::from([
        (
            donder_language::dsl::Identifier::new("gradient".into()).unwrap(),
            Value::Gradient(
                donder_language::values::Gradient {
                    stops: vec![donder_language::values::GradientStop {
                        position: 0.0,
                        color: donder_language::values::Color {
                            red: 255,
                            green: 128,
                            blue: 64,
                        },
                    }],
                }
                .into(),
            ),
        ),
        (
            donder_language::dsl::Identifier::new("pulse_shape".into()).unwrap(),
            Value::Curve(
                donder_language::values::Curve {
                    points: vec![
                        donder_language::values::CurvePoint {
                            position: 0.0,
                            value: 0.0,
                        },
                        donder_language::values::CurvePoint {
                            position: 1.0,
                            value: 1.0,
                        },
                    ],
                }
                .into(),
            ),
        ),
    ]);
    for (case, (name, source, params)) in fixtures::cases()
        .into_iter()
        .chain(fixtures::layer_cases())
        .chain([
            (
                "ArrayLifetimes",
                include_str!(
                    "../../crates/donder-runtime/tests/fixtures/array-lifetimes.effect.donder"
                ),
                indexmap::IndexMap::new(),
            ),
            (
                "Pulse",
                include_str!("../../examples/starter/effects/standard.effect.donder"),
                pulse_params,
            ),
        ])
        .enumerate()
    {
        names.push(name);
        let effect = compile_effects(source).unwrap().remove(0);
        assert_eq!(effect.name().as_str(), name);
        if name == "ArrayLifetimes" {
            assert!(
                effect.sample_program().bytecode().array_capacity > 0,
                "board array-storage coverage was optimized away"
            );
        }
        let bound = donder_language::dsl::bind_params(effect.params(), &params).unwrap();
        let bytecode = effect.sample_program().clone().into_parts().0;
        writeln!(
            generated,
            "fn case_{case}() -> (SampleProgram, BoundParams) {{"
        )
        .unwrap();
        writeln!(generated, "let code = vec![").unwrap();
        for instruction in &bytecode.instructions {
            writeln!(generated, "{},", instruction_source(instruction)).unwrap();
        }
        writeln!(generated, "]; let program = BytecodeProgram {{ instructions: code.into_boxed_slice(), curves: vec![{}].into_boxed_slice(), gradients: vec![{}].into_boxed_slice(), array_constants: vec![{}].into_boxed_slice(), enums: vec![{}].into_boxed_slice(), enum_types: vec![{}].into_boxed_slice(), value_operands: vec![{}].into_boxed_slice(), array_types: vec![{}].into_boxed_slice(), layout: {:?}, uses_pixel_context: {}, pixel_entry: {}, array_capacity: {}, array_width: {}, loop_count: {} }};",
            bytecode.curves.iter().map(|value| curve_source(value)).collect::<Vec<_>>().join(","),
            bytecode.gradients.iter().map(|value| gradient_source(value)).collect::<Vec<_>>().join(","),
            bytecode.array_constants.iter().map(|values| array_source(values)).collect::<Vec<_>>().join(","),
            bytecode.enums.iter().map(identifier_source).collect::<Vec<_>>().join(","),
            bytecode.enum_types.iter().map(|ty| format!("EnumSlotType::new({}).unwrap()", type_source(ty.ty()))).collect::<Vec<_>>().join(","),
            bytecode.value_operands.iter().map(|v| format!("ValueSlot::{v:?}")).collect::<Vec<_>>().join(","),
            bytecode.array_types.iter().map(type_source).collect::<Vec<_>>().join(","),
            bytecode.layout, bytecode.uses_pixel_context, bytecode.pixel_entry, bytecode.array_capacity, bytecode.array_width, bytecode.loop_count).unwrap();
        writeln!(generated, "let types = vec![").unwrap();
        for param in effect.params() {
            writeln!(generated, "{},", type_source(&param.ty)).unwrap();
        }
        writeln!(generated, "]; let values = vec![").unwrap();
        for value in bound.iter_values() {
            writeln!(generated, "{},", value_source(&value)).unwrap();
        }
        writeln!(
            generated,
            "]; let params = BoundParams::bind_values(&types, values).unwrap(); let program = SampleProgram::admit(program, types.into_boxed_slice()).unwrap(); (program, params) }}"
        ).unwrap();
        let mut case_golden = Vec::new();
        for count in workload::COUNTS {
            let show = if case < 4 || name == "ArrayLifetimes" {
                workload::show(count, effect.sample_program().clone(), bound.clone())
            } else {
                workload::layered_show(count, effect.sample_program().clone(), bound.clone(), 16)
            };
            let mut show = show.prepare().into_playback();
            let mut sample = workload::show(count, effect.sample_program().clone(), bound.clone())
                .prepare()
                .into_playback();
            let mut frames = Vec::new();
            let mut gamma_frames = Vec::new();
            for frame in 0..workload::FRAMES {
                let rendered = show.evaluate(workload::time(frame));
                let buffers = rendered.outputs().next().unwrap().bytes;
                // Check max-composition against one prepared sample, then check
                // the GRB route against its fixture colors before recording goldens.
                let sampled = sample.evaluate(workload::time(frame));
                assert_eq!(buffers, sampled.outputs().next().unwrap().bytes);
                for (pixel, color) in sampled.fixtures().next().unwrap().pixels.iter().enumerate() {
                    assert_eq!(
                        &buffers[pixel * 3..pixel * 3 + 3],
                        &[color.green, color.red, color.blue]
                    );
                }
                frames.push(workload::checksum(buffers));
                if case == workload::GAMMA_CASE {
                    assert_eq!(name, "PixelRamp");
                    let bytes = buffers
                        .iter()
                        .map(|&value| gamma_lookup[value as usize])
                        .collect::<Vec<_>>();
                    gamma_frames.push(workload::checksum(&bytes));
                }
            }
            if case == workload::GAMMA_CASE {
                let mut mixed =
                    workload::show(count, effect.sample_program().clone(), bound.clone());
                workload::apply_operator(
                    &mut mixed,
                    identity.program().clone().into_parts().0,
                    true,
                );
                workload::insert_invert(&mut mixed, invert.program().clone().into_parts().0);
                let mut mixed = mixed.prepare().into_playback();
                let mut mixed_frames = Vec::new();
                for frame in 0..workload::FRAMES {
                    let rendered = mixed.evaluate(workload::time(frame));
                    let buffers = rendered.outputs().next().unwrap().bytes;
                    let sampled = sample.evaluate(workload::time(frame));
                    for (pixel, color) in
                        sampled.fixtures().next().unwrap().pixels.iter().enumerate()
                    {
                        assert_eq!(
                            &buffers[pixel * 3..pixel * 3 + 3],
                            &[255 - color.green, 255 - color.red, 255 - color.blue]
                        );
                    }
                    mixed_frames.push(workload::checksum(buffers));
                }
                mixed_golden.push(mixed_frames);
                let mut depths = Vec::new();
                for depth in workload::OPERATOR_DEPTHS {
                    let mut nested =
                        workload::show(count, effect.sample_program().clone(), bound.clone());
                    workload::apply_operator(
                        &mut nested,
                        operator.program().clone().into_parts().0,
                        true,
                    );
                    workload::nest_operator(&mut nested, depth);
                    let nested_fixture = nested;
                    let mut nested = nested_fixture.clone().prepare().into_playback();
                    let mut full_nested =
                        workload::show(count, effect.sample_program().clone(), bound.clone());
                    workload::apply_operator(
                        &mut full_nested,
                        operator.program().clone().into_parts().0,
                        false,
                    );
                    workload::nest_operator(&mut full_nested, depth);
                    let mut full_nested = full_nested.prepare().into_playback();
                    let mut frames = Vec::new();
                    for frame in 0..workload::FRAMES {
                        let rendered = nested.evaluate(workload::time(frame));
                        let buffers = rendered.outputs().next().unwrap().bytes;
                        let checksum = workload::checksum(buffers);
                        let mut fresh = nested_fixture.clone().prepare().into_playback();
                        let rendered = fresh.evaluate(workload::time(frame));
                        let buffers = rendered.outputs().next().unwrap().bytes;
                        assert_eq!(checksum, workload::checksum(buffers));
                        let rendered = full_nested.evaluate(workload::time(frame));
                        let buffers = rendered.outputs().next().unwrap().bytes;
                        assert_eq!(checksum, workload::checksum(buffers));
                        frames.push(checksum);
                    }
                    depths.push(frames);
                }
                nested_golden.push(depths);
                for (empty, golden) in [
                    (false, &mut pulse_automation_golden),
                    (true, &mut empty_golden),
                ] {
                    let mut automated =
                        workload::show(count, effect.sample_program().clone(), bound.clone());
                    workload::apply_pulse_automation(&mut automated, pulse_program.clone(), empty);
                    let automated_fixture = automated;
                    let mut automated = automated_fixture.clone().prepare().into_playback();
                    let mut frames = Vec::new();
                    for frame in 0..workload::FRAMES {
                        let rendered = automated.evaluate(workload::time(frame));
                        let buffers = rendered.outputs().next().unwrap().bytes;
                        let checksum = workload::checksum(buffers);
                        let mut fresh = automated_fixture.clone().prepare().into_playback();
                        let rendered = fresh.evaluate(workload::time(frame));
                        let buffers = rendered.outputs().next().unwrap().bytes;
                        assert_eq!(checksum, workload::checksum(buffers));
                        frames.push(checksum);
                    }
                    golden.push(frames);
                }
                for (pair, golden) in [
                    (
                        [(&operator, false), (&operator, true)],
                        &mut operator_golden,
                    ),
                    (
                        [(&grouped, true), (&alternating, true)],
                        &mut temporal_golden,
                    ),
                ] {
                    let mut expected = None;
                    for (operator, reuse) in pair {
                        let mut show =
                            workload::show(count, effect.sample_program().clone(), bound.clone());
                        workload::apply_operator(
                            &mut show,
                            operator.program().clone().into_parts().0,
                            reuse,
                        );
                        let mut show = show.prepare().into_playback();
                        let mut frames = Vec::new();
                        for frame in 0..workload::FRAMES {
                            let rendered = show.evaluate(workload::time(frame));
                            let buffers = rendered.outputs().next().unwrap().bytes;
                            frames.push(workload::checksum(buffers));
                        }
                        if let Some(expected) = &expected {
                            assert_eq!(&frames, expected);
                        } else {
                            expected = Some(frames);
                        }
                    }
                    golden.push(expected.unwrap());
                }
                {
                    let mut show = workload::layered_show(
                        count,
                        effect.sample_program().clone(),
                        bound.clone(),
                        1,
                    );
                    workload::apply_gamma(&mut show, gamma_lookup);
                    let mut show = show.prepare().into_playback();
                    for (frame, expected) in gamma_frames.iter().enumerate() {
                        let rendered = show.evaluate(workload::time(frame));
                        let buffers = rendered.outputs().next().unwrap().bytes;
                        assert_eq!(workload::checksum(buffers), *expected);
                    }
                }
                gamma_golden.push(gamma_frames);
            }
            case_golden.push(frames);
        }
        golden.push(case_golden);
    }
    writeln!(
        generated,
        "pub const NESTED_GOLDEN: [[[u32; {}]; 3]; 4] = {nested_golden:?};",
        workload::FRAMES
    )
    .unwrap();
    writeln!(
        generated,
        "pub const EMPTY_GOLDEN: [[u32; {}]; 4] = {empty_golden:?};",
        workload::FRAMES
    )
    .unwrap();
    writeln!(
        generated,
        "pub const MIXED_GOLDEN: [[u32; {}]; 4] = {mixed_golden:?};",
        workload::FRAMES
    )
    .unwrap();
    for (name, operator) in [
        ("identity_program", &identity),
        ("invert_program", &invert),
        ("operator_program", &operator),
        ("grouped_program", &grouped),
        ("alternating_program", &alternating),
    ] {
        let bytecode = operator.program().clone().into_parts().0;
        writeln!(generated, "pub fn {name}() -> BytecodeProgram {{ BytecodeProgram {{ instructions: vec![{}].into(), curves: vec![{}].into(), gradients: vec![{}].into(), array_constants: vec![{}].into(), enums: vec![{}].into(), enum_types: vec![{}].into(), value_operands: vec![{}].into(), array_types: vec![{}].into(), layout: {:?}, uses_pixel_context: {}, pixel_entry: {}, array_capacity: {}, array_width: {}, loop_count: {} }} }}",
        bytecode.instructions.iter().map(instruction_source).collect::<Vec<_>>().join(","),
        bytecode.curves.iter().map(|value| curve_source(value)).collect::<Vec<_>>().join(","),
        bytecode.gradients.iter().map(|value| gradient_source(value)).collect::<Vec<_>>().join(","),
        bytecode.array_constants.iter().map(|values| array_source(values)).collect::<Vec<_>>().join(","),
        bytecode.enums.iter().map(identifier_source).collect::<Vec<_>>().join(","),
        bytecode.enum_types.iter().map(|ty| format!("EnumSlotType::new({}).unwrap()", type_source(ty.ty()))).collect::<Vec<_>>().join(","),
        bytecode.value_operands.iter().map(|v| format!("ValueSlot::{v:?}")).collect::<Vec<_>>().join(","),
        bytecode.array_types.iter().map(type_source).collect::<Vec<_>>().join(","),
        bytecode.layout, bytecode.uses_pixel_context, bytecode.pixel_entry, bytecode.array_capacity, bytecode.array_width, bytecode.loop_count).unwrap();
    }
    writeln!(
        generated,
        "pub const TEMPORAL_GOLDEN: [[u32; {}]; 4] = {temporal_golden:?};",
        workload::FRAMES
    )
    .unwrap();
    writeln!(
        generated,
        "pub const PULSE_AUTOMATION_GOLDEN: [[u32; {}]; 4] = {pulse_automation_golden:?};",
        workload::FRAMES
    )
    .unwrap();
    writeln!(
        generated,
        "pub const OPERATOR_GOLDEN: [[u32; {}]; 4] = {operator_golden:?};",
        workload::FRAMES
    )
    .unwrap();
    writeln!(
        generated,
        "pub const GAMMA_LOOKUP: [u8; 256] = {gamma_lookup:?};"
    )
    .unwrap();
    writeln!(
        generated,
        "pub const GAMMA_GOLDEN: [[u32; {}]; 4] = {gamma_golden:?};",
        workload::FRAMES
    )
    .unwrap();
    writeln!(
        generated,
        "pub const NAMES: [&str; {}] = {names:?};",
        names.len()
    )
    .unwrap();
    writeln!(
        generated,
        "pub const GOLDEN: [[[u32; {}]; 4]; {}] = {golden:?};",
        workload::FRAMES,
        names.len()
    )
    .unwrap();
    writeln!(
        generated,
        "pub fn case(index: usize) -> (SampleProgram, BoundParams) {{ match index {{"
    )
    .unwrap();
    for case in 0..names.len() {
        writeln!(generated, "{case} => case_{case}(),").unwrap();
    }
    writeln!(generated, "_ => panic!(\"invalid case\") }} }}").unwrap();
    let chase_pulse_golden = workload::CHASE_PULSE_CASES.map(|(name, layers)| {
        let show = workload::chase_pulse_show(200, layers);
        export_fixture(name, &show)
    });
    writeln!(
        generated,
        "#[allow(dead_code)] pub const CHASE_PULSE_GOLDEN: [[u32; {}]; {}] = {chase_pulse_golden:?};",
        workload::FRAMES,
        workload::CHASE_PULSE_CASES.len()
    )
    .unwrap();
    writeln!(
        generated,
        "#[allow(dead_code)] pub const CHASE_PULSE_SEQUENCES: [&[u8]; {}] = [",
        workload::CHASE_PULSE_CASES.len()
    )
    .unwrap();
    for (name, _) in workload::CHASE_PULSE_CASES {
        writeln!(
            generated,
            "include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{name}.donderseq\")),"
        )
        .unwrap();
    }
    writeln!(generated, "];").unwrap();
    let mark_golden = workload::MARK_CASES.map(|(name, pulse)| {
        let show = mark_workload::mark_show(200, pulse);
        export_fixture(name, &show)
    });
    writeln!(
        generated,
        "#[allow(dead_code)] pub const MARK_GOLDEN: [[u32; {}]; {}] = {mark_golden:?};",
        workload::FRAMES,
        workload::MARK_CASES.len()
    )
    .unwrap();
    writeln!(
        generated,
        "#[allow(dead_code)] pub const MARK_SEQUENCES: [&[u8]; {}] = [",
        workload::MARK_CASES.len()
    )
    .unwrap();
    for (name, _) in workload::MARK_CASES {
        writeln!(
            generated,
            "include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{name}.donderseq\")),"
        )
        .unwrap();
    }
    writeln!(generated, "];").unwrap();
    fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("fixtures.rs"),
        generated,
    )
    .unwrap();
}

fn export_fixture(name: &str, show: &workload::Workload) -> [u32; workload::FRAMES] {
    let directory = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let prepared = show.clone().prepare();
    let bytes = donder_runtime::encode_sequence(&prepared).unwrap();
    let decoded = donder_runtime::decode_sequence(&bytes, Default::default()).unwrap();
    fs::write(directory.join(format!("{name}.donderseq")), bytes).unwrap();
    assert_eq!(
        prepared
            .outputs()
            .iter()
            .map(|output| output.width)
            .collect::<Vec<_>>(),
        [600]
    );
    let mut decoded = decoded.into_playback();
    let mut checksums = String::new();
    let golden = core::array::from_fn(|frame| {
        let time = workload::time(frame);
        let rendered = decoded.evaluate(time);
        let output = rendered.outputs().next().unwrap().bytes;
        let mut fresh = show.clone().prepare().into_playback();
        let rendered_reference = fresh.evaluate(time);
        let reference = rendered_reference.outputs().next().unwrap().bytes;
        assert_eq!(output, reference);
        assert!(output.iter().any(|&byte| byte != 0));
        writeln!(checksums, "{} {}", time.as_ticks(), crc32fast::hash(output)).unwrap();
        workload::checksum(output)
    });
    fs::write(
        directory.join(format!("{name}.donderseq.checksums")),
        checksums,
    )
    .unwrap();
    golden
}

// This is build-only Rust source emission for the benchmark fixtures, not a
// serialized show format. Unsupported resource values fail the build explicitly.
fn type_source(ty: &Type) -> String {
    match ty {
        Type::Enum(options) => format!(
            "Type::Enum(vec![{}])",
            options
                .iter()
                .map(|v| format!("Identifier::new({:?}.into()).unwrap()", v.as_str()))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Array(item) => format!("Type::Array(Box::new({}))", type_source(item)),
        _ => format!("Type::{ty:?}"),
    }
}

fn identifier_source(value: &donder_language::dsl::Identifier) -> String {
    format!("Identifier::new({:?}.into()).unwrap()", value.as_str())
}

fn array_source(values: &[Value]) -> String {
    format!(
        "Arc::from(vec![{}])",
        values
            .iter()
            .map(value_source)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn value_source(value: &Value) -> String {
    match value {
        Value::Marks(value) => format!("Value::Marks({})", marks_source(value)),
        Value::Enum(v) => format!(
            "Value::Enum(Identifier::new({:?}.into()).unwrap())",
            v.as_str()
        ),
        Value::Curve(v) => format!("Value::Curve({})", curve_source(v)),
        Value::Gradient(v) => format!("Value::Gradient({})", gradient_source(v)),
        Value::Array(v) => format!("Value::Array({})", array_source(v)),
        Value::Void | Value::Int(_) | Value::Float(_) | Value::Bool(_) | Value::Color(_) => {
            format!("Value::{value:?}")
        }
    }
}

fn curve_source(value: &donder_language::values::Curve) -> String {
    format!("Arc::new(Curve {{ points: vec!{:?} }})", value.points)
}

fn gradient_source(value: &donder_language::values::Gradient) -> String {
    format!("Arc::new(Gradient {{ stops: vec!{:?} }})", value.stops)
}

fn marks_source(value: &donder_language::values::Marks) -> String {
    let marks = value
        .marks
        .iter()
        .map(|mark| {
            format!(
                "donder_language::values::SampleDuration::from_ticks({})",
                mark.as_ticks()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("Arc::new(donder_language::values::Marks {{ marks: vec![{marks}] }})")
}

fn instruction_source(instruction: &Instruction) -> String {
    if let Instruction::LoadMarksConst { dst, value } = instruction {
        return format!(
            "Instruction::LoadMarksConst {{ dst: {dst:?}, value: {} }}",
            marks_source(value)
        );
    }
    let mut source = format!("Instruction::{instruction:?}");
    for slot in [
        "Int", "Float", "Bool", "Color", "Array", "Marks", "Curve", "Gradient", "Enum",
    ] {
        for prefix in [" ", "("] {
            source = source.replace(
                &format!("{prefix}{slot}({slot}Slot("),
                &format!("{prefix}ValueSlot::{slot}({slot}Slot("),
            );
        }
    }
    // Numeric operands have their own restricted slot union.
    for field in ["index", "width", "seconds", "fallback"] {
        source = source
            .replace(
                &format!("{field}: ValueSlot::"),
                &format!("{field}: NumberSlot::"),
            )
            .replace(
                &format!("{field}: Some(ValueSlot::"),
                &format!("{field}: Some(NumberSlot::"),
            );
    }
    if matches!(instruction, Instruction::ContextRead { .. }) {
        source = source.replace("dst: ValueSlot::", "dst: NumberSlot::");
    }
    let op_type = match instruction {
        Instruction::FloatArithmetic { .. } | Instruction::FloatArithmeticConst { .. } => {
            Some("ArithmeticOp")
        }
        Instruction::IntArithmetic { .. } => Some("IntArithmeticOp"),
        Instruction::IntCompare { .. }
        | Instruction::FloatCompare { .. }
        | Instruction::FloatCompareConst { .. } => Some("CompareOp"),
        Instruction::FloatUnary { .. } => Some("FloatUnary"),
        Instruction::FloatBinary { .. } | Instruction::FloatBinaryConst { .. } => {
            Some("FloatBinary")
        }
        Instruction::ColorBinary { .. } => Some("ColorBinary"),
        Instruction::ColorComponent { .. } => Some("ColorComponent"),
        Instruction::Mark { .. } => Some("MarkOp"),
        _ => None,
    };
    if let Some(op_type) = op_type {
        source = source.replace("op: ", &format!("op: {op_type}::"));
    }
    source
        .replace("dst: Void", "dst: ValueSlot::Void")
        .replace("read: ", "read: ContextRead::")
        .replace("pixel: ", "pixel: SignalPixel::")
}
