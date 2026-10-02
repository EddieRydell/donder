use super::*;
use crate::dsl::bytecode::{
    ArithmeticOp, ArraySlot, ContextRead, FloatSlot, Instruction, NumberSlot, PoolSpan, SlotLayout,
    ValueSlot,
};
use crate::dsl::{CalculationProgram, DslBindCache, Value};
use alloc::vec;

/// Equivalent to `[seed + seconds()]`, followed by `[item; 8]` and
/// `[repeated; 4]`. Each parent array is forwarded repeatedly into one child.
fn array_calculation(input: Type, repetitions: u32) -> PreparedParameterCalculation {
    let output = Type::array(input.clone());
    let (mut instructions, item, destination, array_types, layout) = if input == Type::Float {
        (
            vec![
                Instruction::LoadFloatParam {
                    dst: FloatSlot(0),
                    param: 0,
                    source: FloatSlot(0),
                },
                Instruction::ContextRead {
                    dst: NumberSlot::Float(FloatSlot(1)),
                    read: ContextRead::Seconds,
                },
                Instruction::FloatArithmetic {
                    dst: FloatSlot(2),
                    op: ArithmeticOp::Add,
                    left: FloatSlot(0),
                    right: FloatSlot(1),
                },
            ],
            ValueSlot::Float(FloatSlot(2)),
            ArraySlot(0),
            vec![output.clone()],
            SlotLayout {
                floats: 3,
                arrays: 1,
                ..SlotLayout::default()
            },
        )
    } else {
        (
            vec![Instruction::LoadArrayParam {
                dst: ArraySlot(0),
                param: 0,
                source: ArraySlot(0),
            }],
            ValueSlot::Array(ArraySlot(0)),
            ArraySlot(1),
            vec![input.clone(), output.clone()],
            SlotLayout {
                arrays: 2,
                ..SlotLayout::default()
            },
        )
    };
    let mut operands = vec![item; repetitions as usize];
    operands.push(ValueSlot::Array(destination));
    instructions.extend([
        Instruction::MakeArray {
            dst: destination,
            items: PoolSpan {
                start: 0,
                len: repetitions,
            },
        },
        Instruction::ReturnValues(PoolSpan {
            start: repetitions,
            len: 1,
        }),
    ]);
    let mut program = BytecodeProgram {
        instructions: instructions.into(),
        array_constants: Box::new([]),
        enums: Box::new([]),
        enum_types: Box::new([]),
        curves: Box::new([]),
        targets: Box::new([]),
        target_lists: Box::new([]),
        target_items: Box::new([]),
        gradients: Box::new([]),
        value_operands: operands.into(),
        array_types: array_types.into(),
        layout,
        uses_pixel_context: false,
        pixel_entry: 0,
        array_capacity: 2,
        array_width: repetitions,
        loop_count: 0,
    };
    (program.array_capacity, program.array_width) = program.required_array_storage().unwrap();
    let admitted =
        CalculationProgram::new(program, vec![input].into(), vec![output].into()).unwrap();
    let (program, _, outputs) = admitted.into_parts();
    PreparedParameterCalculation { program, outputs }
}

#[test]
fn retained_array_copies_preserve_sharing_across_nested_calculations() {
    let mut cache = DslBindCache::default();
    let mut environments: Vec<PreparedParameterEnvironment> = vec![PreparedParameterEnvironment {
        start_time: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        params: BoundParams::from_values([(&Type::Float, Value::Float(0.25))], &mut cache),
        types: vec![Type::Float].into(),
        bindings: Box::new([]),
        automation: Box::new([]),
        calculation: None,
        array_capacity: 0,
        array_width: 0,
    }];
    let mut input = Type::Float;
    for (parent, repetitions) in [1, 8, 4].into_iter().enumerate() {
        let calculation = array_calculation(input.clone(), repetitions);
        let (array_capacity, array_width) = PreparedParameterEnvironment::required_array_storage(
            [&environments[parent]],
            Some(&calculation),
        );
        environments.push(PreparedParameterEnvironment {
            start_time: SampleTime::from_ticks(0),
            duration: SampleDuration::from_ticks(1_000_000),
            params: BoundParams::from_values([(&input, Value::Void)], &mut cache),
            types: vec![input.clone()].into(),
            bindings: vec![PreparedParameterBinding {
                parameter: 0,
                source: ParameterSource {
                    environment: parent,
                    parameter: 0,
                },
            }]
            .into(),
            automation: Box::new([]),
            calculation: Some(calculation),
            array_capacity,
            array_width,
        });
        input = Type::array(input);
    }
    let environments = admit_environments(environments).unwrap();
    // The admitted arena cannot hold the 37 nodes produced by expanding all aliases.
    // Correct evaluation therefore depends on sharing surviving every copy.
    assert!(environments[3].array_capacity < 37);
    for time_slots in [1, 2] {
        let mut workspace = ParameterWorkspace::new(&environments, time_slots);
        for time in [0, 500_000, 200_000, 0, 500_000] {
            let leaf = Value::Array(vec![Value::Float(0.25 + time as f32 / 1_000_000.0)].into());
            let repeated = Value::Array(vec![leaf; 8].into());
            let expected = Value::Array(vec![repeated; 4].into());
            let output = workspace.resolve(&environments, 3, SampleTime::from_ticks(time));
            assert_eq!(output.value(0).unwrap(), expected);
        }
    }
}
