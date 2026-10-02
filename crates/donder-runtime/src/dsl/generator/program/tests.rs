use super::*;
use crate::dsl::bytecode::{
    BytecodeProgram, Instruction, IntSlot, MarksSlot, PoolSpan, SlotLayout, TargetSlot, ValueSlot,
};

fn calculation<O: super::super::super::CalculationOutput>(
    instruction: Instruction,
    inputs: &[Type],
    output: ValueSlot,
    ty: Type,
    layout: SlotLayout,
) -> CalculationProgram<O> {
    CalculationProgram::new(
        BytecodeProgram {
            instructions: vec![
                instruction,
                Instruction::ReturnValues(PoolSpan { start: 0, len: 1 }),
            ]
            .into(),
            array_constants: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            curves: Box::new([]),
            targets: Box::new([]),
            target_lists: Box::new([]),
            target_items: Box::new([]),
            gradients: Box::new([]),
            value_operands: vec![output].into(),
            array_types: Box::new([]),
            layout,
            uses_pixel_context: false,
            pixel_entry: 0,
            array_capacity: 0,
            array_width: 0,
            loop_count: 0,
        },
        inputs.into(),
        vec![ty].into(),
    )
    .unwrap()
    .into_output()
    .unwrap()
}

fn integer<O: super::super::super::CalculationOutput>(value: i32) -> Box<FixedCalculation<O>> {
    Box::new(FixedCalculation {
        program: calculation(
            Instruction::LoadIntConst {
                dst: IntSlot(0),
                value,
            },
            &[],
            ValueSlot::Int(IntSlot(0)),
            Type::Int,
            SlotLayout {
                ints: 1,
                ..SlotLayout::default()
            },
        ),
        inputs: Box::new([]),
    })
}

fn empty_loops(index: BindingSlot) -> [Statement; 2] {
    [
        Statement::Range {
            index,
            count: integer(0),
            cap: 1,
            body: vec![],
        },
        Statement::Marks {
            index,
            marks: Box::new(FixedCalculation {
                program: calculation(
                    Instruction::LoadMarksConst {
                        dst: MarksSlot(0),
                        value: Arc::new(Marks { marks: vec![] }),
                    },
                    &[],
                    ValueSlot::Marks(MarksSlot(0)),
                    Type::Marks,
                    SlotLayout {
                        marks: 1,
                        ..SlotLayout::default()
                    },
                ),
                inputs: Box::new([]),
            }),
            body: vec![],
        },
    ]
}

#[test]
fn empty_loops_do_not_initialize_fresh_indices() {
    let accepted = empty_loops(BindingSlot(2)).map(|loop_statement| {
        GeneratorProgram::admit(
            vec![],
            vec![
                loop_statement,
                Statement::Expression(Expression::Read(BindingSlot(2))),
            ],
            vec![Type::Target, Type::Float, Type::Int].into(),
            Box::new([]),
        )
        .is_some()
    });
    assert_eq!(
        accepted,
        [false, false],
        "Range and Marks must preserve the uninitialized zero-iteration path"
    );
}

#[test]
fn empty_loops_preserve_existing_values_and_live_bindings() {
    let name = Identifier::new("value".into()).unwrap();
    let emit = Statement::Emit {
        slot: GeneratedEffectSlot(0),
        start: integer(0),
        duration: integer(1),
        target: Box::new(FixedCalculation {
            program: calculation(
                Instruction::LoadTargetParam {
                    dst: TargetSlot(0),
                    param: 0,
                    source: TargetSlot(0),
                },
                &[Type::Target],
                ValueSlot::Target(TargetSlot(0)),
                Type::Target,
                SlotLayout {
                    targets: 1,
                    ..SlotLayout::default()
                },
            ),
            inputs: vec![FixedBindingSlot(BindingSlot(1))].into(),
        }),
        params: vec![(name.clone(), Expression::Read(BindingSlot(0)))],
    };
    let context = GeneratorContext {
        start_time: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        target: Arc::new(super::super::super::TargetValue::default()),
    };
    for loop_statement in empty_loops(BindingSlot(0)) {
        for fixed in [true, false] {
            let declaration = ParamDecl {
                fixed,
                name: name.clone(),
                ty: Type::Int,
                default: None,
            };
            let body = vec![loop_statement.clone(), emit.clone()];
            let slots: Box<[_]> = vec![Type::Int, Type::Target, Type::Float].into();
            let program = GeneratorProgram::admit(
                vec![declaration.clone()],
                body.clone(),
                slots.clone(),
                vec![vec![declaration.clone()].into_boxed_slice()].into(),
            )
            .unwrap();
            let inputs = if fixed {
                [GeneratorInput::Fixed(Value::Int(7))]
            } else {
                [GeneratorInput::Live]
            };
            let result = program.bind(&inputs).unwrap().specialize(&context);
            assert_eq!(result.children.len(), 1);
            let expected = if fixed {
                GeneratorBinding::Constant(Value::Int(7))
            } else {
                GeneratorBinding::Parameter(0)
            };
            assert_eq!(result.children[0].params, vec![(name.clone(), expected)]);

            if !fixed {
                let fixed_output = ParamDecl {
                    fixed: true,
                    ..declaration.clone()
                };
                assert!(
                    GeneratorProgram::admit(
                        vec![declaration],
                        body,
                        slots,
                        vec![vec![fixed_output].into_boxed_slice()].into(),
                    )
                    .is_none(),
                    "{loop_statement:?}: an empty loop cannot make an incoming live binding fixed"
                );
            }
        }
    }
}
