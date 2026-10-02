use super::*;
use crate::automation::AutomationMapping;
use crate::dsl::bytecode::{ArithmeticOp, TargetSlot};
use crate::dsl::generator::{BindingSlot, Calculation, Expression, FixedBindingSlot};
use crate::dsl::{Identifier, ParamDecl};
use crate::sequence::PreparedSequence;
use crate::values::{Curve, CurvePoint};

fn level() -> ParamDecl {
    ParamDecl {
        fixed: false,
        name: Identifier::new("level".into()).unwrap(),
        ty: Type::Float,
        default: None,
    }
}

fn sample(gain: f32) -> Shared<SampleProgram> {
    Shared::new(
        SampleProgram::admit(
            raw_program(
                vec![
                    Instruction::LoadFloatParam {
                        dst: FloatSlot(0),
                        param: 0,
                        source: FloatSlot(0),
                    },
                    Instruction::FloatArithmeticConst {
                        dst: FloatSlot(0),
                        op: ArithmeticOp::Multiply,
                        value: FloatSlot(0),
                        constant_bits: gain.to_bits(),
                        constant_left: false,
                    },
                    Instruction::Rgb {
                        dst: ColorSlot(0),
                        red: FloatSlot(0),
                        green: FloatSlot(0),
                        blue: FloatSlot(0),
                    },
                    Instruction::ReturnColor(ColorSlot(0)),
                ],
                SlotLayout {
                    floats: 1,
                    colors: 1,
                    ..SlotLayout::default()
                },
            ),
            vec![Type::Float].into(),
        )
        .unwrap(),
    )
}

fn sample_target(program: &Shared<SampleProgram>) -> GeneratorTarget {
    GeneratorTarget::Sample {
        program: Shared::clone(program),
        params: vec![level()].into(),
    }
}

fn half_level() -> Expression {
    let mut raw = raw_program(
        vec![
            Instruction::LoadFloatParam {
                dst: FloatSlot(0),
                param: 0,
                source: FloatSlot(0),
            },
            Instruction::FloatArithmeticConst {
                dst: FloatSlot(0),
                op: ArithmeticOp::Multiply,
                value: FloatSlot(0),
                constant_bits: 0.5_f32.to_bits(),
                constant_left: false,
            },
            Instruction::ReturnValues(PoolSpan { start: 0, len: 1 }),
        ],
        SlotLayout {
            floats: 1,
            ..SlotLayout::default()
        },
    );
    raw.value_operands = vec![ValueSlot::Float(FloatSlot(0))].into();
    Expression::Calculate(Box::new(Calculation {
        program: CalculationProgram::new(raw, vec![Type::Float].into(), vec![Type::Float].into())
            .unwrap()
            .into_output()
            .unwrap(),
        inputs: vec![BindingSlot(0)].into(),
    }))
}

fn emitting(
    targets: Vec<GeneratorTarget>,
    slot: GeneratedEffectSlot,
    value: Expression,
) -> Shared<LinkedGenerator> {
    let mut target = raw_program(
        vec![
            Instruction::LoadTargetParam {
                dst: TargetSlot(0),
                param: 0,
                source: TargetSlot(0),
            },
            Instruction::ReturnValues(PoolSpan { start: 0, len: 1 }),
        ],
        SlotLayout {
            targets: 1,
            ..SlotLayout::default()
        },
    );
    target.value_operands = vec![ValueSlot::Target(TargetSlot(0))].into();
    let program = GeneratorProgram::admit(
        vec![level()],
        vec![Statement::Emit {
            slot,
            start: fixed_seconds(0.0),
            duration: fixed_seconds(1.0),
            target: Box::new(FixedCalculation {
                program: CalculationProgram::new(
                    target,
                    vec![Type::Target].into(),
                    vec![Type::Target].into(),
                )
                .unwrap()
                .into_output()
                .unwrap(),
                inputs: vec![FixedBindingSlot(BindingSlot(1))].into(),
            }),
            params: vec![(level().name, value)],
        }],
        vec![Type::Float, Type::Target, Type::Float].into(),
        vec![vec![level()].into_boxed_slice(); targets.len()].into(),
    )
    .unwrap();
    LinkedGenerator::link(Shared::new(program), targets.into()).unwrap()
}

fn ramp() -> Box<[PreparedAutomation]> {
    vec![PreparedAutomation {
        start: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        curve: Shared::new(Curve {
            points: vec![
                CurvePoint {
                    position: 0.0,
                    value: 0.0,
                },
                CurvePoint {
                    position: 1.0,
                    value: 1.0,
                },
            ],
        }),
        mapping: AutomationMapping::Float { min: 0.0, max: 1.0 },
        param_index: 0,
    }]
    .into()
}

fn sequence(
    generator: Shared<LinkedGenerator>,
    automation: Box<[PreparedAutomation]>,
) -> PreparedSequence {
    let playback = GeneratorPlayback::admit(
        generator,
        vec![Value::Float(0.5)],
        automation,
        &mut DslBindCache::default(),
    )
    .unwrap();
    let timing = SequenceTiming::admit(
        NonZeroU32::new(60).unwrap(),
        NonZeroU32::new(60).unwrap(),
        NonZeroU32::new(1_000_000).unwrap(),
        Box::new([]),
    )
    .unwrap();
    PreparedSequence::build(timing, |builder| {
        let fixture = builder.fixture(0, FixtureGeometry::admit(vec![[0.0, 0.0]].into()).unwrap());
        let target = builder.target([fixture], TargetScope::WholeTarget);
        let children = builder.generator(&playback, builder.whole_sequence(), target);
        assert_eq!(children.len(), 1);
        let layer = builder.layer(true, children.iter().map(|child| child.effect));
        builder.output([layer])
    })
}

#[test]
fn constant_generator_drops_all_environments_and_freezes_dsl_leaf() {
    let generator = emitting(
        vec![sample_target(&sample(1.0))],
        GeneratedEffectSlot(0),
        Expression::Read(BindingSlot(0)),
    );
    let sequence = sequence(generator, Box::new([]));
    let graph = sequence.archive_data().signals;
    assert!(graph.parameter_environments.is_empty());
    assert_eq!(graph.effects.len(), 1);
    assert!(matches!(
        graph.effects[0].implementation,
        PreparedEffectImplementation::Dsl { .. }
    ));
    let mut playback = sequence.into_playback();
    let first = playback.evaluate(SampleTime::from_ticks(0)).colors()[0];
    assert!(first.red > 0);
    assert_eq!(
        first,
        playback.evaluate(SampleTime::from_ticks(750_000)).colors()[0]
    );
}

#[test]
fn automated_generator_retains_environments_and_bound_leaf() {
    let generator = emitting(
        vec![sample_target(&sample(1.0))],
        GeneratedEffectSlot(0),
        Expression::Read(BindingSlot(0)),
    );
    let sequence = sequence(generator, ramp());
    let graph = sequence.archive_data().signals;
    assert!(!graph.parameter_environments.is_empty());
    assert_eq!(graph.effects.len(), 1);
    let PreparedEffectImplementation::Bound { environment, .. } = graph.effects[0].implementation
    else {
        panic!("live emitted sample must retain its parameter environment");
    };
    let leaf = &graph.parameter_environments[environment];
    assert_eq!(leaf.bindings.len(), 1);
    let root = &graph.parameter_environments[leaf.bindings[0].source.environment];
    assert_eq!(root.automation.len(), 1);
    let mut playback = sequence.into_playback();
    let early = playback.evaluate(SampleTime::from_ticks(250_000)).colors()[0];
    let late = playback.evaluate(SampleTime::from_ticks(750_000)).colors()[0];
    assert!(late.red > early.red);
    assert_eq!(
        early,
        playback.evaluate(SampleTime::from_ticks(250_000)).colors()[0]
    );
}

#[test]
fn nested_live_generator_retains_the_complete_calculation_dependency_chain() {
    let sample = sample(1.0);
    let inner = emitting(
        vec![sample_target(&sample)],
        GeneratedEffectSlot(0),
        half_level(),
    );
    let outer = emitting(
        vec![GeneratorTarget::Generator(inner)],
        GeneratedEffectSlot(0),
        half_level(),
    );
    let sequence = sequence(outer, ramp());
    let graph = sequence.archive_data().signals;
    assert_eq!(graph.effects.len(), 1);
    let PreparedEffectImplementation::Bound { environment, .. } = graph.effects[0].implementation
    else {
        panic!("nested live sample must retain its dependency chain");
    };
    let environments = &graph.parameter_environments;
    let leaf = &environments[environment];
    assert!(leaf.calculation.is_none());
    assert_eq!(leaf.bindings.len(), 1);
    let inner_index = leaf.bindings[0].source.environment;
    let inner = &environments[inner_index];
    assert!(inner.calculation.is_some());
    assert_eq!(inner.bindings.len(), 1);
    let outer_index = inner.bindings[0].source.environment;
    let outer = &environments[outer_index];
    assert!(outer.calculation.is_some());
    assert_eq!(outer.bindings.len(), 1);
    let root_index = outer.bindings[0].source.environment;
    let root = &environments[root_index];
    assert!(root.calculation.is_none());
    assert!(root.bindings.is_empty());
    assert_eq!(root.automation.len(), 1);
    assert!(root_index < outer_index && outer_index < inner_index && inner_index < environment);

    // Two generator calculations must equal one sample-side quarter gain.
    let reference = emitting(
        vec![sample_target(&self::sample(0.25))],
        GeneratedEffectSlot(0),
        Expression::Read(BindingSlot(0)),
    );
    let mut reference = self::sequence(reference, ramp()).into_playback();
    let mut playback = sequence.into_playback();
    for ticks in [250_000, 750_000, 250_000] {
        let time = SampleTime::from_ticks(ticks);
        assert_eq!(
            playback.evaluate(time).colors(),
            reference.evaluate(time).colors()
        );
    }
}

#[test]
fn emitted_child_uses_the_sample_program_linked_at_its_chosen_slot() {
    let unused = sample(0.25);
    let chosen = sample(1.0);
    let generator = emitting(
        vec![sample_target(&unused), sample_target(&chosen)],
        GeneratedEffectSlot(1),
        Expression::Read(BindingSlot(0)),
    );
    for automation in [Box::new([]) as Box<[_]>, ramp()] {
        let sequence = sequence(Shared::clone(&generator), automation);
        let graph = &sequence.data.signals;
        assert_eq!(graph.effects.len(), 1);
        let program = graph.effects[0].implementation.dsl_program();
        assert_eq!(graph.programs.sample(program), chosen.as_ref());
        assert_ne!(graph.programs.sample(program), unused.as_ref());
    }
}
