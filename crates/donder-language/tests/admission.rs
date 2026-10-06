//! Admission of strip bytecode: `BytecodeProgram::is_well_formed` and the
//! effect and operator admissions built on it accept minimal programs and
//! reject every malformed slot, block, nesting, limit, pool, parameter and
//! signal reference.
use donder_language::Shared;
use donder_language::dsl::bytecode::{
    Bank, Banks, BytecodeProgram, CompareOp, ContextRead, FloatBinary, FloatUnary, Input,
    Instruction, MAX_DEPTH, MAX_ROW_BYTES, NO_FRAME_CACHE, ParameterKind, ProgramContext, Reducer,
    Resource, SignalPixel, Slot, Span,
};
use donder_language::dsl::{OperatorProgram, SampleProgram, Type};
use donder_language::values::{Color, Curve, Gradient, Marks};

const RED: Color = Color {
    red: 255,
    green: 0,
    blue: 0,
};

fn banks(bank: Bank, count: u16) -> Banks {
    let mut banks = Banks::default();
    *banks.get_mut(bank) = count;
    banks
}

/// `banks` with `count` slots of `bank`.
fn with(mut banks: Banks, bank: Bank, count: u16) -> Banks {
    *banks.get_mut(bank) = count;
    banks
}

/// A program without pools whose first `prefix` instructions are its query
/// block and the rest its body.
fn program(
    code: Vec<Instruction>,
    prefix: u16,
    result: Slot,
    scalars: Banks,
    rows: Banks,
) -> BytecodeProgram {
    BytecodeProgram {
        code: code.into(),
        query_end: prefix,
        target_end: prefix,
        result,
        scalars,
        rows,
        depth: 0,
        curves: Box::new([]),
        gradients: Box::new([]),
        marks: Box::new([]),
        arrays: Box::new([]),
        enums: Box::new([]),
        operands: Box::new([]),
        frame_caches: 0,
    }
}

/// Whether `program` admits as an effect over parameters of `params`' types;
/// `is_well_formed` agrees.
fn effect(program: &BytecodeProgram, params: &[Type]) -> bool {
    let kinds: Vec<_> = params.iter().map(ParameterKind::for_type).collect();
    let admitted = SampleProgram::admit(program.clone(), params.into()).is_some();
    assert_eq!(
        admitted,
        program.is_well_formed(ProgramContext::Effect, &kinds)
    );
    admitted
}

/// Whether `program` admits as an operator over `inputs` signals.
fn operator(program: &BytecodeProgram, inputs: usize, params: &[Type]) -> bool {
    let kinds: Vec<_> = params.iter().map(ParameterKind::for_type).collect();
    let admitted = OperatorProgram::admit(program.clone(), inputs, params.into()).is_some();
    assert_eq!(
        admitted,
        program.is_well_formed(ProgramContext::Operator { inputs }, &kinds)
    );
    admitted
}

/// A constant color, loaded in the query block.
fn constant() -> BytecodeProgram {
    program(
        vec![Instruction::ColorConst {
            dst: Slot::scalar(0),
            value: RED,
        }],
        1,
        Slot::scalar(0),
        banks(Bank::Color, 1),
        Banks::default(),
    )
}

/// A color of every pixel's position, computed in the body.
fn positions() -> BytecodeProgram {
    program(
        vec![Instruction::Rgb {
            dst: Slot::row(0),
            red: Slot::input(Input::PixelFraction),
            green: Slot::input(Input::PixelX),
            blue: Slot::input(Input::PixelY),
        }],
        0,
        Slot::row(0),
        Banks::default(),
        banks(Bank::Color, 1),
    )
}

/// An operator's first input at the query's time, frame cached.
fn sampled() -> BytecodeProgram {
    BytecodeProgram {
        frame_caches: 1,
        ..program(
            vec![
                Instruction::Context {
                    dst: Slot::scalar(0),
                    read: ContextRead::Seconds,
                },
                Instruction::Sample {
                    dst: Slot::row(0),
                    input: 0,
                    seconds: Slot::scalar(0),
                    pixel: SignalPixel::Current,
                    frame_cache: 0,
                },
            ],
            1,
            Slot::row(0),
            banks(Bank::Float, 1),
            banks(Bank::Color, 1),
        )
    }
}

/// `then_color` where a pixel's fraction is below a half, `else_color`
/// elsewhere: one selection.
fn branched() -> BytecodeProgram {
    BytecodeProgram {
        depth: 1,
        ..program(
            vec![
                Instruction::FloatConst {
                    dst: Slot::scalar(0),
                    bits: 0.5f32.to_bits(),
                },
                Instruction::ColorConst {
                    dst: Slot::scalar(0),
                    value: RED,
                },
                Instruction::ColorConst {
                    dst: Slot::scalar(1),
                    value: Color::BLACK,
                },
                Instruction::FloatCompare {
                    op: CompareOp::Less,
                    dst: Slot::row(0),
                    a: Slot::input(Input::PixelFraction),
                    b: Slot::scalar(0),
                },
                Instruction::Branch {
                    condition: Slot::row(0),
                    then_len: 1,
                    else_len: 1,
                },
                Instruction::Move {
                    bank: Bank::Color,
                    dst: Slot::row(0),
                    src: Slot::scalar(0),
                },
                Instruction::Move {
                    bank: Bank::Color,
                    dst: Slot::row(0),
                    src: Slot::scalar(1),
                },
            ],
            3,
            Slot::row(0),
            with(banks(Bank::Float, 1), Bank::Color, 2),
            with(banks(Bank::Bool, 1), Bank::Color, 1),
        )
    }
}

/// The sum of `0..4`, as a gray, by a scalar reduction.
fn reduced() -> BytecodeProgram {
    program(
        vec![
            Instruction::IntConst {
                dst: Slot::scalar(0),
                value: 0,
            },
            Instruction::IntConst {
                dst: Slot::scalar(1),
                value: 4,
            },
            Instruction::IntConst {
                dst: Slot::scalar(2),
                value: 0,
            },
            Instruction::Reduce {
                reducer: Reducer::Sum,
                bank: Bank::Int,
                acc: Slot::scalar(2),
                index: Slot::scalar(3),
                start: Slot::scalar(0),
                end: Slot::scalar(1),
                filter: Slot::NONE,
                value: Slot::scalar(3),
                loop_len: 0,
                contribute_len: 0,
            },
            Instruction::IntToFloat {
                dst: Slot::scalar(0),
                a: Slot::scalar(2),
            },
            Instruction::Rgb {
                dst: Slot::scalar(0),
                red: Slot::scalar(0),
                green: Slot::scalar(0),
                blue: Slot::scalar(0),
            },
        ],
        3,
        Slot::scalar(0),
        with(with(banks(Bank::Int, 4), Bank::Float, 1), Bank::Color, 1),
        Banks::default(),
    )
}

/// `program` with `edit` applied to its code.
fn edited(
    mut program: BytecodeProgram,
    edit: impl FnOnce(&mut Vec<Instruction>),
) -> BytecodeProgram {
    let mut code = program.code.into_vec();
    edit(&mut code);
    program.code = code.into();
    program
}

#[test]
fn minimal_programs_are_admitted() {
    for program in [constant(), positions(), branched(), reduced()] {
        assert!(effect(&program, &[]), "{program:?}");
        assert!(operator(&program, 0, &[]), "{program:?}");
    }
    assert!(operator(&sampled(), 1, &[]));
    let uncached = edited(sampled(), |code| {
        if let Instruction::Sample { frame_cache, .. } = &mut code[1] {
            *frame_cache = NO_FRAME_CACHE;
        }
    });
    assert!(operator(&uncached, 1, &[]));
    assert!(operator(
        &BytecodeProgram {
            frame_caches: 0,
            ..uncached
        },
        1,
        &[]
    ));
}

#[test]
fn slots_beyond_their_bank_and_kind_are_rejected() {
    let curve = Shared::new(Curve { points: vec![] });
    for bank in Bank::ALL {
        let dst = Slot::scalar(0);
        let load = match bank {
            Bank::Float => Instruction::FloatConst { dst, bits: 0 },
            Bank::Int => Instruction::IntConst { dst, value: 0 },
            Bank::Bool => Instruction::BoolConst { dst, value: false },
            Bank::Color => Instruction::ColorConst { dst, value: RED },
            Bank::Resource => Instruction::ResourceConst {
                dst,
                kind: Resource::Curve,
                index: 0,
            },
        };
        // A scalar of `bank` broadcast into its row.
        let base = BytecodeProgram {
            curves: Box::new([curve.clone()]),
            ..program(
                vec![
                    Instruction::ColorConst { dst, value: RED },
                    load,
                    Instruction::Move {
                        bank,
                        dst: Slot::row(0),
                        src: Slot::scalar(0),
                    },
                ],
                2,
                Slot::scalar(0),
                with(banks(Bank::Color, 1), bank, 1),
                banks(bank, 1),
            )
        };
        assert!(effect(&base, &[]), "{bank:?}");
        let scalars = with(base.scalars, bank, 0);
        assert!(
            !effect(
                &BytecodeProgram {
                    scalars,
                    ..base.clone()
                },
                &[]
            ),
            "{bank:?}"
        );
        let rows = with(base.rows, bank, 0);
        assert!(
            !effect(
                &BytecodeProgram {
                    rows,
                    ..base.clone()
                },
                &[]
            ),
            "{bank:?}"
        );
        // Row indices count rows, not scalars.
        let row_source = edited(base.clone(), |code| {
            code[2] = Instruction::Move {
                bank,
                dst: Slot::row(0),
                src: Slot::row(1),
            };
        });
        let scalars = with(base.scalars, bank, 2);
        assert!(
            !effect(
                &BytecodeProgram {
                    scalars,
                    ..row_source
                },
                &[]
            ),
            "{bank:?}"
        );
    }
    // The result is a color slot of the program.
    for result in [
        Slot::scalar(1),
        Slot::row(0),
        Slot::input(Input::PixelFraction),
    ] {
        assert!(!effect(
            &BytecodeProgram {
                result,
                ..constant()
            },
            &[]
        ));
    }
}

#[test]
fn pixel_inputs_are_read_only_and_keep_their_banks() {
    let rewrite = |instruction: Instruction| {
        edited(positions(), |code| {
            code.insert(0, instruction);
        })
    };
    let float_row = BytecodeProgram {
        rows: with(banks(Bank::Color, 1), Bank::Float, 1),
        ..positions()
    };
    assert!(effect(
        &edited(float_row.clone(), |code| code.insert(
            0,
            Instruction::FloatUnary {
                op: FloatUnary::Abs,
                dst: Slot::row(0),
                a: Slot::input(Input::PixelX),
            }
        )),
        &[]
    ));
    for instruction in [
        // Written.
        Instruction::FloatUnary {
            op: FloatUnary::Abs,
            dst: Slot::input(Input::PixelFraction),
            a: Slot::input(Input::PixelX),
        },
        Instruction::Move {
            bank: Bank::Float,
            dst: Slot::input(Input::PixelY),
            src: Slot::input(Input::PixelX),
        },
        Instruction::IntNegate {
            dst: Slot::input(Input::PixelIndex),
            a: Slot::input(Input::PixelIndex),
        },
        // Read from another bank.
        Instruction::FloatUnary {
            op: FloatUnary::Abs,
            dst: Slot::row(0),
            a: Slot::input(Input::PixelIndex),
        },
        Instruction::IntToFloat {
            dst: Slot::row(0),
            a: Slot::input(Input::PixelFraction),
        },
        Instruction::Invert {
            dst: Slot::row(0),
            color: Slot::input(Input::PixelX),
        },
    ] {
        let program = BytecodeProgram {
            rows: float_row.rows,
            ..rewrite(instruction.clone())
        };
        assert!(!effect(&program, &[]), "{instruction:?}");
    }
    // A constant written into a pixel input.
    assert!(!effect(
        &rewrite(Instruction::FloatConst {
            dst: Slot::input(Input::PixelFraction),
            bits: 0,
        }),
        &[]
    ));
}

#[test]
fn destinations_are_rows_exactly_when_an_operand_is() {
    let scalars = with(banks(Bank::Float, 1), Bank::Int, 1);
    let rows = with(banks(Bank::Color, 1), Bank::Float, 1);
    let one = |instruction: Instruction| BytecodeProgram {
        scalars,
        rows: with(rows, Bank::Int, 1),
        ..edited(positions(), |code| code.insert(0, instruction))
    };
    let accepted = [
        Instruction::FloatUnary {
            op: FloatUnary::Abs,
            dst: Slot::row(0),
            a: Slot::input(Input::PixelFraction),
        },
        Instruction::FloatUnary {
            op: FloatUnary::Abs,
            dst: Slot::scalar(0),
            a: Slot::scalar(0),
        },
        // A move broadcasts a scalar into a row.
        Instruction::Move {
            bank: Bank::Float,
            dst: Slot::row(0),
            src: Slot::scalar(0),
        },
        // Section queries always vary by pixel.
        Instruction::SectionCount {
            dst: Slot::row(0),
            width: Slot::scalar(0),
        },
    ];
    for instruction in accepted {
        assert!(effect(&one(instruction.clone()), &[]), "{instruction:?}");
    }
    let rejected = [
        Instruction::FloatUnary {
            op: FloatUnary::Abs,
            dst: Slot::scalar(0),
            a: Slot::input(Input::PixelFraction),
        },
        Instruction::FloatUnary {
            op: FloatUnary::Abs,
            dst: Slot::row(0),
            a: Slot::scalar(0),
        },
        Instruction::FloatBinary {
            op: FloatBinary::Add,
            dst: Slot::scalar(0),
            a: Slot::scalar(0),
            b: Slot::row(0),
        },
        Instruction::Move {
            bank: Bank::Float,
            dst: Slot::scalar(0),
            src: Slot::row(0),
        },
        Instruction::SectionCount {
            dst: Slot::scalar(0),
            width: Slot::scalar(0),
        },
        Instruction::FloatConst {
            dst: Slot::row(0),
            bits: 0,
        },
    ];
    for instruction in rejected {
        assert!(!effect(&one(instruction.clone()), &[]), "{instruction:?}");
    }
    // Samples always write rows.
    let scalar_sample = edited(sampled(), |code| {
        if let Instruction::Sample { dst, .. } = &mut code[1] {
            *dst = Slot::scalar(0);
        }
    });
    let scalar_sample = BytecodeProgram {
        result: Slot::scalar(0),
        scalars: with(scalar_sample.scalars, Bank::Color, 1),
        ..scalar_sample
    };
    assert!(!operator(&scalar_sample, 1, &[]));
}

#[test]
fn reductions_keep_row_accumulators_and_matching_indices() {
    let reduction = |acc, index, start, value| {
        edited(reduced(), |code| {
            code[3] = Instruction::Reduce {
                reducer: Reducer::Sum,
                bank: Bank::Int,
                acc,
                index,
                start,
                end: Slot::scalar(1),
                filter: Slot::NONE,
                value,
                loop_len: 0,
                contribute_len: 0,
            };
        })
    };
    let rows = |program: BytecodeProgram| BytecodeProgram {
        rows: banks(Bank::Int, 2),
        depth: 1,
        ..program
    };
    let scalar = (Slot::scalar(2), Slot::scalar(3), Slot::scalar(0));
    assert!(effect(&reduced(), &[]));
    // A row accumulator of scalar operands: its default varies by pixel.
    // The program's result stays scalar, so the row is unused afterwards.
    let row_acc = rows(reduction(Slot::row(0), scalar.1, scalar.2, scalar.1));
    assert!(effect(&row_acc, &[]));
    // A scalar accumulator of a varying value.
    let varying = rows(reduction(
        scalar.0,
        scalar.1,
        scalar.2,
        Slot::input(Input::PixelIndex),
    ));
    assert!(!effect(&varying, &[]));
    // Per-pixel bounds need a row index, and only they do.
    let row_bounds = |index| {
        rows(reduction(
            Slot::row(0),
            index,
            Slot::input(Input::PixelIndex),
            index,
        ))
    };
    assert!(effect(&row_bounds(Slot::row(1)), &[]));
    assert!(!effect(&row_bounds(Slot::scalar(3)), &[]));
    assert!(!effect(
        &rows(reduction(Slot::row(0), Slot::row(1), scalar.2, scalar.1)),
        &[]
    ));
    assert!(!effect(
        &rows(reduction(
            Slot::row(0),
            Slot::input(Input::PixelIndex),
            Slot::input(Input::PixelIndex),
            Slot::scalar(3),
        )),
        &[]
    ));
    // A reducer combines only the banks it is defined for.
    let any_int = edited(reduced(), |code| {
        if let Instruction::Reduce { reducer, .. } = &mut code[3] {
            *reducer = Reducer::Any;
        }
    });
    assert!(!effect(&any_int, &[]));
}

#[test]
fn prefix_blocks_hold_only_scalar_code_without_pixel_queries() {
    // The position color computed in the query block, or the target block.
    for (query_end, target_end) in [(1, 1), (0, 1)] {
        let program = BytecodeProgram {
            query_end,
            target_end,
            ..positions()
        };
        assert!(!effect(&program, &[]), "{query_end}..{target_end}");
    }
    // A sample in the query block.
    let sample_in_query = BytecodeProgram {
        query_end: 2,
        target_end: 2,
        ..sampled()
    };
    assert!(!operator(&sample_in_query, 1, &[]));
    // A section query with scalar operands, in the target block.
    let section = BytecodeProgram {
        query_end: 1,
        target_end: 2,
        ..program_with_section()
    };
    assert!(!effect(&section, &[]));
    assert!(effect(&program_with_section(), &[]));
    // A branch on a scalar condition is scalar code.
    let scalar_branch = program(
        vec![
            Instruction::BoolConst {
                dst: Slot::scalar(0),
                value: true,
            },
            Instruction::Branch {
                condition: Slot::scalar(0),
                then_len: 1,
                else_len: 1,
            },
            Instruction::ColorConst {
                dst: Slot::scalar(0),
                value: RED,
            },
            Instruction::ColorConst {
                dst: Slot::scalar(0),
                value: Color::BLACK,
            },
        ],
        4,
        Slot::scalar(0),
        with(banks(Bank::Bool, 1), Bank::Color, 1),
        Banks::default(),
    );
    assert!(effect(&scalar_branch, &[]));
    // Blocks are ordered and within the code.
    for (query_end, target_end) in [(2, 1), (1, 2)] {
        let program = BytecodeProgram {
            query_end,
            target_end,
            ..constant()
        };
        assert!(!effect(&program, &[]), "{query_end}..{target_end}");
    }
}

/// A section count of a scalar width, in the body.
fn program_with_section() -> BytecodeProgram {
    program(
        vec![
            Instruction::IntConst {
                dst: Slot::scalar(0),
                value: 3,
            },
            Instruction::SectionCount {
                dst: Slot::row(0),
                width: Slot::scalar(0),
            },
            Instruction::IntToFloat {
                dst: Slot::row(0),
                a: Slot::row(0),
            },
            Instruction::Rgb {
                dst: Slot::row(0),
                red: Slot::row(0),
                green: Slot::row(0),
                blue: Slot::row(0),
            },
        ],
        1,
        Slot::row(0),
        banks(Bank::Int, 1),
        with(with(banks(Bank::Int, 1), Bank::Float, 1), Bank::Color, 1),
    )
}

#[test]
fn nested_code_stays_within_its_block_and_construct() {
    // Branch arms past the end of the code: two instructions follow.
    for (then_len, else_len) in [(3, 0), (1, 2), (0, 3)] {
        let program = edited(branched(), |code| {
            code[4] = Instruction::Branch {
                condition: Slot::row(0),
                then_len,
                else_len,
            };
        });
        assert!(!effect(&program, &[]), "{then_len}+{else_len}");
    }
    // A reduction's parts past the end of the code, with two instructions
    // after it or none.
    for (loop_len, contribute_len, len) in [(3, 0, 6), (0, 3, 6), (2, 1, 6), (1, 0, 4), (0, 1, 4)] {
        let program = edited(reduced(), |code| {
            if let Instruction::Reduce {
                loop_len: parts,
                contribute_len: contribute,
                ..
            } = &mut code[3]
            {
                (*parts, *contribute) = (loop_len, contribute_len);
            }
            code.truncate(len);
        });
        assert!(
            !effect(&program, &[]),
            "{loop_len}+{contribute_len} of {len}"
        );
    }
    // A branch whose arm holds a branch longer than the arm.
    let inner = edited(branched(), |code| {
        code[4] = Instruction::Branch {
            condition: Slot::row(0),
            then_len: 2,
            else_len: 0,
        };
        code.insert(
            5,
            Instruction::Branch {
                condition: Slot::row(0),
                then_len: 2,
                else_len: 0,
            },
        );
    });
    assert!(!effect(&BytecodeProgram { depth: 2, ..inner }, &[]));
    // A scalar branch in the query block whose arms run into the body.
    let straddling = program(
        vec![
            Instruction::BoolConst {
                dst: Slot::scalar(0),
                value: true,
            },
            Instruction::Branch {
                condition: Slot::scalar(0),
                then_len: 1,
                else_len: 0,
            },
            Instruction::ColorConst {
                dst: Slot::scalar(0),
                value: RED,
            },
        ],
        2,
        Slot::scalar(0),
        with(banks(Bank::Bool, 1), Bank::Color, 1),
        Banks::default(),
    );
    assert!(!effect(&straddling, &[]));
    assert!(effect(
        &BytecodeProgram {
            query_end: 3,
            target_end: 3,
            ..straddling
        },
        &[]
    ));
}

/// `count` branches on a row condition, each the only code of the last's
/// then arm, around a move of red into the result.
fn nested_branches(count: u16) -> BytecodeProgram {
    let mut code = vec![
        Instruction::FloatConst {
            dst: Slot::scalar(0),
            bits: 0.5f32.to_bits(),
        },
        Instruction::ColorConst {
            dst: Slot::scalar(0),
            value: RED,
        },
        Instruction::FloatCompare {
            op: CompareOp::Less,
            dst: Slot::row(0),
            a: Slot::input(Input::PixelFraction),
            b: Slot::scalar(0),
        },
        Instruction::Move {
            bank: Bank::Color,
            dst: Slot::row(0),
            src: Slot::scalar(0),
        },
    ];
    for depth in 0..count {
        code.insert(
            3,
            Instruction::Branch {
                condition: Slot::row(0),
                then_len: depth + 1,
                else_len: 0,
            },
        );
    }
    BytecodeProgram {
        depth: count,
        ..program(
            code,
            2,
            Slot::row(0),
            with(banks(Bank::Float, 1), Bank::Color, 1),
            with(banks(Bank::Bool, 1), Bank::Color, 1),
        )
    }
}

#[test]
fn selections_fit_the_declared_and_maximum_depth() {
    assert!(effect(&nested_branches(1), &[]));
    assert!(effect(&nested_branches(MAX_DEPTH), &[]));
    assert!(!effect(&nested_branches(MAX_DEPTH + 1), &[]));
    // Code deeper than the program declares.
    assert!(!effect(
        &BytecodeProgram {
            depth: 2,
            ..nested_branches(3)
        },
        &[]
    ));
    assert!(!effect(
        &BytecodeProgram {
            depth: 0,
            ..branched()
        },
        &[]
    ));
    // A declared depth beyond the maximum, whatever the code needs.
    assert!(!effect(
        &BytecodeProgram {
            depth: MAX_DEPTH + 1,
            ..constant()
        },
        &[]
    ));
    // Scalar conditions select nothing.
    let scalar = edited(nested_branches(MAX_DEPTH + 1), |code| {
        for instruction in code {
            if let Instruction::Branch { condition, .. } = instruction {
                *condition = Slot::scalar(0);
            }
        }
    });
    let scalar = BytecodeProgram {
        depth: 0,
        scalars: with(scalar.scalars, Bank::Bool, 1),
        ..scalar
    };
    assert!(effect(&scalar, &[]));
}

#[test]
fn rows_fit_the_row_byte_limit() {
    // The result's color row, float rows, and bool rows to the limit.
    let spare = MAX_ROW_BYTES - Bank::Color.row_bytes();
    let floats = spare / Bank::Float.row_bytes();
    let bools = spare - floats * Bank::Float.row_bytes();
    let program = |bools: u32| BytecodeProgram {
        rows: Banks {
            floats: floats as u16,
            bools: bools as u16,
            colors: 1,
            ..Banks::default()
        },
        ..positions()
    };
    assert_eq!(program(bools).rows.row_bytes(), MAX_ROW_BYTES);
    assert!(effect(&program(bools), &[]));
    assert!(!effect(&program(bools + 1), &[]));
}

#[test]
fn resource_constants_and_picks_stay_within_their_pools() {
    for kind in [
        Resource::Curve,
        Resource::Gradient,
        Resource::Marks,
        Resource::Array,
    ] {
        let load = program(
            vec![
                Instruction::ColorConst {
                    dst: Slot::scalar(0),
                    value: RED,
                },
                Instruction::ResourceConst {
                    dst: Slot::scalar(0),
                    kind,
                    index: 0,
                },
            ],
            2,
            Slot::scalar(0),
            with(banks(Bank::Color, 1), Bank::Resource, 1),
            Banks::default(),
        );
        assert!(!effect(&load, &[]), "{kind:?}");
        let mut pooled = load.clone();
        match kind {
            Resource::Curve => pooled.curves = Box::new([Shared::new(Curve { points: vec![] })]),
            Resource::Gradient => {
                pooled.gradients = Box::new([Shared::new(Gradient { stops: vec![] })])
            }
            Resource::Marks => pooled.marks = Box::new([Shared::new(Marks::EMPTY)]),
            Resource::Array => pooled.arrays = Box::new([Shared::from(Vec::new())]),
        }
        assert!(effect(&pooled, &[]), "{kind:?}");
        let past = edited(pooled, |code| {
            code[1] = Instruction::ResourceConst {
                dst: Slot::scalar(0),
                kind,
                index: 1,
            };
        });
        assert!(!effect(&past, &[]), "{kind:?}");
    }
    let pick = |items: Span, operands: Vec<Slot>| BytecodeProgram {
        operands: operands.into(),
        ..program(
            vec![
                Instruction::IntConst {
                    dst: Slot::scalar(0),
                    value: 0,
                },
                Instruction::ColorConst {
                    dst: Slot::scalar(1),
                    value: RED,
                },
                Instruction::Pick {
                    bank: Bank::Color,
                    dst: Slot::scalar(0),
                    index: Slot::scalar(0),
                    items,
                },
            ],
            3,
            Slot::scalar(0),
            with(banks(Bank::Int, 1), Bank::Color, 2),
            Banks::default(),
        )
    };
    let item = || vec![Slot::scalar(1)];
    assert!(effect(&pick(Span { start: 0, len: 1 }, item()), &[]));
    // No items.
    assert!(!effect(&pick(Span { start: 0, len: 0 }, item()), &[]));
    // Items past the operand pool.
    assert!(!effect(&pick(Span { start: 0, len: 2 }, item()), &[]));
    assert!(!effect(&pick(Span { start: 1, len: 1 }, item()), &[]));
    // Pooled items name slots of the pick's bank.
    assert!(!effect(
        &pick(Span { start: 0, len: 1 }, vec![Slot::scalar(2)]),
        &[]
    ));
    assert!(!effect(
        &pick(Span { start: 0, len: 1 }, vec![Slot::input(Input::PixelX)]),
        &[]
    ));
}

#[test]
fn parameter_reads_stay_within_their_bound_banks() {
    let enum_type = Type::Enum(vec![
        donder_language::dsl::Identifier::new("a".into()).unwrap(),
    ]);
    let array_type = Type::Array(Box::new(Type::Float));
    let dst = Slot::scalar(0);
    // Each read, the type it reads, and a type of another bank.
    for (read, ty, other, bank) in [
        (
            Instruction::FloatParam { dst, bank: 0 },
            Type::Float,
            Type::Int,
            Bank::Float,
        ),
        (
            Instruction::IntParam { dst, bank: 0 },
            Type::Int,
            Type::Float,
            Bank::Int,
        ),
        (
            Instruction::BoolParam { dst, bank: 0 },
            Type::Bool,
            Type::Color,
            Bank::Bool,
        ),
        (
            Instruction::ColorParam { dst, bank: 0 },
            Type::Color,
            Type::Bool,
            Bank::Color,
        ),
        (
            Instruction::EnumParam { dst, bank: 0 },
            enum_type.clone(),
            Type::Int,
            Bank::Int,
        ),
        (
            Instruction::ResourceParam {
                dst,
                kind: Resource::Curve,
                bank: 0,
            },
            Type::Curve,
            Type::Gradient,
            Bank::Resource,
        ),
        (
            Instruction::ResourceParam {
                dst,
                kind: Resource::Gradient,
                bank: 0,
            },
            Type::Gradient,
            Type::Marks,
            Bank::Resource,
        ),
        (
            Instruction::ResourceParam {
                dst,
                kind: Resource::Marks,
                bank: 0,
            },
            Type::Marks,
            array_type.clone(),
            Bank::Resource,
        ),
        (
            Instruction::ResourceParam {
                dst,
                kind: Resource::Array,
                bank: 0,
            },
            array_type.clone(),
            Type::Curve,
            Bank::Resource,
        ),
    ] {
        let program = program(
            vec![
                Instruction::ColorConst {
                    dst: Slot::scalar(0),
                    value: RED,
                },
                read.clone(),
            ],
            2,
            Slot::scalar(0),
            with(banks(Bank::Color, 1), bank, 1),
            Banks::default(),
        );
        assert!(effect(&program, std::slice::from_ref(&ty)), "{read:?}");
        assert!(
            operator(&program, 0, &[other.clone(), ty.clone()]),
            "{read:?}"
        );
        assert!(!effect(&program, &[]), "{read:?}");
        assert!(!effect(&program, std::slice::from_ref(&other)), "{read:?}");
        // The second parameter of the kind.
        let second = edited(program, |code| {
            let mut read = read.clone();
            match &mut read {
                Instruction::FloatParam { bank, .. }
                | Instruction::IntParam { bank, .. }
                | Instruction::BoolParam { bank, .. }
                | Instruction::ColorParam { bank, .. }
                | Instruction::EnumParam { bank, .. }
                | Instruction::ResourceParam { bank, .. } => *bank = 1,
                _ => unreachable!("parameter reads"),
            }
            code[1] = read;
        });
        assert!(!effect(&second, &[ty.clone(), other.clone()]), "{read:?}");
        assert!(effect(&second, &[ty.clone(), other, ty]), "{read:?}");
    }
}

#[test]
fn signals_are_read_only_by_operators_within_their_inputs_and_caches() {
    // Samples and fused clocks need an operator.
    assert!(!effect(&sampled(), &[]));
    let clock = program(
        vec![
            Instruction::Context {
                dst: Slot::scalar(0),
                read: ContextRead::Seconds,
            },
            Instruction::Clock {
                progress: true,
                dst: Slot::scalar(1),
                seconds: Slot::scalar(0),
            },
            Instruction::Rgb {
                dst: Slot::scalar(0),
                red: Slot::scalar(1),
                green: Slot::scalar(1),
                blue: Slot::scalar(1),
            },
        ],
        3,
        Slot::scalar(0),
        with(banks(Bank::Float, 2), Bank::Color, 1),
        Banks::default(),
    );
    assert!(!effect(&clock, &[]));
    assert!(operator(&clock, 0, &[]));
    // The input index is within the operator's inputs.
    assert!(!operator(&sampled(), 0, &[]));
    let second = edited(sampled(), |code| {
        if let Instruction::Sample { input, .. } = &mut code[1] {
            *input = 1;
        }
    });
    assert!(!operator(&second, 1, &[]));
    assert!(operator(&second, 2, &[]));
    // The frame cache is one of the program's.
    assert!(!operator(
        &BytecodeProgram {
            frame_caches: 0,
            ..sampled()
        },
        1,
        &[]
    ));
    let cache = |frame_cache| {
        edited(sampled(), |code| {
            if let Instruction::Sample {
                frame_cache: cache, ..
            } = &mut code[1]
            {
                *cache = frame_cache;
            }
        })
    };
    assert!(!operator(&cache(1), 1, &[]));
    assert!(operator(
        &BytecodeProgram {
            frame_caches: 2,
            ..cache(1)
        },
        1,
        &[]
    ));
    // Addressed pixels are int slots of the program.
    let addressed = |pixel| {
        edited(sampled(), |code| {
            if let Instruction::Sample { pixel: address, .. } = &mut code[1] {
                *address = pixel;
            }
        })
    };
    assert!(operator(
        &addressed(SignalPixel::Local(Slot::input(Input::PixelIndex))),
        1,
        &[]
    ));
    assert!(!operator(
        &addressed(SignalPixel::Global(Slot::input(Input::PixelFraction))),
        1,
        &[]
    ));
    assert!(!operator(
        &addressed(SignalPixel::Global(Slot::scalar(0))),
        1,
        &[]
    ));
}

#[test]
fn resources_are_not_compared() {
    let program = BytecodeProgram {
        curves: Box::new([Shared::new(Curve { points: vec![] })]),
        ..program(
            vec![
                Instruction::ResourceConst {
                    dst: Slot::scalar(0),
                    kind: Resource::Curve,
                    index: 0,
                },
                Instruction::Equal {
                    bank: Bank::Resource,
                    negate: false,
                    dst: Slot::scalar(0),
                    a: Slot::scalar(0),
                    b: Slot::scalar(0),
                },
                Instruction::ColorConst {
                    dst: Slot::scalar(0),
                    value: RED,
                },
            ],
            3,
            Slot::scalar(0),
            with(
                with(banks(Bank::Resource, 1), Bank::Bool, 1),
                Bank::Color,
                1,
            ),
            Banks::default(),
        )
    };
    assert!(!effect(&program, &[]));
    let floats = edited(program, |code| {
        code[0] = Instruction::FloatConst {
            dst: Slot::scalar(0),
            bits: 0,
        };
        code[1] = Instruction::Equal {
            bank: Bank::Float,
            negate: false,
            dst: Slot::scalar(0),
            a: Slot::scalar(0),
            b: Slot::scalar(0),
        };
    });
    let floats = BytecodeProgram {
        scalars: with(floats.scalars, Bank::Float, 1),
        ..floats
    };
    assert!(effect(&floats, &[]));
}
