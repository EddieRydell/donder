use donder_language::Shared;
use donder_language::dsl::{
    BindingError, Invocation, ParamDecl, ProgramConstants, Type, Value, compile_effects,
    compile_operators,
};
use donder_language::execution::PreparedAutomation;
use donder_language::values::{
    Color, Curve, CurvePoint, Gradient, GradientStop, Marks, SampleDuration, SampleTime,
};

/// Guards, reductions with parameter and array bounds, array literals,
/// shadowing lets and integer powers.
const EFFECT_TEMPLATE: &str = "effect Probe {
  param amount: float in 0.0..1.0 = 0.5;
  param count: int in 1..8 = 3;
  param mode: enum { a, b } = a;
  param colors: array<gradient>;
  sample {
    let x = if mode == b { 1.0 - pixel.fraction } else { pixel.fraction };
    let x = x * amount;
    guard x > 0.1;
    let level = [0.25, 0.5, 1.0][pixel.index % 3];
    guard level < 0.9 else #ff0000;
    max for i in 0..count { colors[i][x] * pow(amount, i) * level }
  }
}";

/// Current, temporal, local and global signal samples, inside and outside
/// reductions.
const OPERATOR_TEMPLATE: &str = "operator Probe {
  input source;
  input other;
  param gain: float in 0.0..1.0 = 0.5;
  param taps: int in 1..4 = 2;
  sample {
    let mirrored = source.at(time - gain, target.count - 1 - pixel.index);
    let global = other.at_global(time, pixel.index + 1);
    let echo = sum for tap in 1..=taps { other.at(time - gain * tap) * pow(gain, tap) };
    guard intensity(source) < 0.99 else source;
    max(max(source, mirrored), global + echo)
  }
}";

/// Event queries, `first`/`last`/`any`/`all` reductions bounded by `len()`,
/// curves, sections and randomness.
const MARKS_TEMPLATE: &str = "effect Probe {
  param beats: marks;
  param shape: curve in 0.0..1.0;
  param tint: color = #20ff40;
  param decay: float in 0.01..10.0 = 0.5;
  param seed: float in 0.0..1000.0 = 0.0;
  sample {
    let hit = mark_last(beats, time);
    guard !is_nan(hit);
    let age = time - hit;
    guard age >= 0.0 && age < decay;
    let next = first for i in 0..len(beats) { guard mark_at(beats, i) > hit; mark_at(beats, i) } else { duration };
    let previous = last for i in 0..len(beats) { guard mark_at(beats, i) < hit; i } else { -1 };
    let lit = (any for i in 0..len(beats) { mark_at(beats, i) <= time }) && (all for j in 0..=2 { j < 3 });
    guard lit;
    let section = section_index(3);
    let sparkle = rand((seed + hit * 1000.0) * 31.0 + section);
    hsv(hue(tint) + sparkle, 1.0, shape[age / decay]) * value_or(next - hit, 1.0) * (previous + 2)
  }
}";

#[derive(Clone, Copy)]
enum Kind {
    Effect,
    Operator,
}

#[test]
fn templates_compile_and_lower() {
    for (template, kind) in templates() {
        // Empty, populated and automated values, each with and without
        // instance constants.
        assert_eq!(compile_and_lower(template, kind), Some(6), "{template}");
    }
}

/// Interchangeable words. Swapping one for another usually keeps a source
/// well formed, so these mutations reach checking, instantiation and lowering.
const SWAPS: &[&[&str]] = &[
    &["+", "-", "*", "/", "%"],
    &["<", ">", "<=", ">=", "==", "!="],
    &["&&", "||"],
    &["..", "..="],
    &["max", "min", "sum", "first", "last", "any", "all"],
    &[
        "time",
        "progress",
        "duration",
        "pixel.index",
        "pixel.fraction",
        "pixel.x",
        "target.count",
        "target.max_y",
    ],
    &["0", "1", "3", "0.0", "0.1", "0.5", "1.0", "-2.5"],
    &["source", "other"],
];

#[test]
fn malformed_source_never_panics_during_compilation_or_lowering() {
    let alphabet = b"{}()[];,:.<>!=+-*/%#&|abcin0 9\n";
    let mut state = 0x51a7_d5e3_u64;
    let mut random = move |bound: usize| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (state >> 33) as usize % bound
    };
    let mut lowered = 0;
    for (template, kind) in templates() {
        for case in 0..800 {
            let mut source = template.to_owned();
            for _ in 0..(case % 5 + 1) {
                if case % 2 == 0 {
                    // Edit one byte; the templates are ASCII.
                    let position = random(source.len());
                    let character = char::from(alphabet[random(alphabet.len())]);
                    match random(3) {
                        0 => source.replace_range(position..=position, &character.to_string()),
                        1 => source.insert(position, character),
                        _ => {
                            source.remove(position);
                        }
                    }
                } else {
                    let words = SWAPS[random(SWAPS.len())];
                    let sites: Vec<(usize, &str)> = words
                        .iter()
                        .flat_map(|word| source.match_indices(word))
                        .filter(|&(start, word)| is_whole_word(&source, start, word))
                        .collect();
                    if !sites.is_empty() {
                        let (start, word) = sites[random(sites.len())];
                        let replacement = words[random(words.len())];
                        source.replace_range(start..start + word.len(), replacement);
                    }
                }
            }
            match std::panic::catch_unwind(|| compile_and_lower(&source, kind)) {
                Ok(Some(instances)) if instances > 0 => lowered += 1,
                Ok(_) => {}
                Err(_) => panic!("compiler panicked for mutation {case}: {source}"),
            }
        }
    }
    assert!(lowered > 100, "only {lowered} mutations reached lowering");
}

/// Whether a word-like match is not part of a longer name or number.
fn is_whole_word(source: &str, start: usize, word: &str) -> bool {
    let part = |character: char| character.is_ascii_alphanumeric() || character == '_';
    if !word.starts_with(part) {
        return true;
    }
    !source[..start].ends_with(part) && !source[start + word.len()..].starts_with(part)
}

fn templates() -> [(&'static str, Kind); 3] {
    [
        (EFFECT_TEMPLATE, Kind::Effect),
        (OPERATOR_TEMPLATE, Kind::Operator),
        (MARKS_TEMPLATE, Kind::Effect),
    ]
}

/// Compile `source` and lower every instance of the definitions it declares,
/// as preparation would, including (for operators) after black-input folding
/// and fusion. Returns the number of instances lowered, or `None` when the
/// source does not compile.
fn compile_and_lower(source: &str, kind: Kind) -> Option<usize> {
    let mut lowered = 0;
    match kind {
        Kind::Effect => {
            for effect in compile_effects(source).ok()? {
                let invoke = |values, automation| effect.invoke(values, automation);
                for invocation in invocations(effect.params(), invoke) {
                    for constants in constants() {
                        let instance = invocation.instance(constants);
                        let _ = instance.sample();
                        let _ = instance.explain();
                        lowered += 1;
                    }
                }
            }
        }
        Kind::Operator => {
            for operator in compile_operators(source).ok()? {
                let invoke = |values, automation| operator.invoke(values, automation);
                for invocation in invocations(operator.params(), invoke) {
                    for constants in constants() {
                        let instance = invocation.instance(constants);
                        let _ = instance.operator();
                        let _ = instance.explain();
                        for input in 0..instance.inputs() {
                            let _ = instance.with_black_input(input).operator();
                            if let Some(fused) = instance.fuse_input(input, &instance) {
                                let _ = fused.operator();
                            }
                        }
                        lowered += 1;
                    }
                }
            }
        }
    }
    Some(lowered)
}

/// Bind declared defaults with empty and with populated required values, and
/// once more with every automatable parameter automated. A mutated range may
/// reject a value; that is a binding error, not a panic.
fn invocations(
    params: &[ParamDecl],
    invoke: impl Fn(Vec<Value>, Box<[PreparedAutomation]>) -> Result<Invocation, BindingError>,
) -> Vec<Invocation> {
    let values = |populated: bool| {
        params
            .iter()
            .map(|param| match &param.default {
                Some(value) => value.clone(),
                None if populated => populated_value(&param.ty),
                None => param.ty.default_value(),
            })
            .collect::<Vec<_>>()
    };
    let automation = params
        .iter()
        .enumerate()
        .filter_map(|(index, param)| {
            Some(PreparedAutomation {
                start: SampleTime::from_ticks(0),
                duration: SampleDuration::from_ticks(2_000_000),
                curve: Shared::new(ramp()),
                mapping: param.automation_mapping()?,
                param_index: index as u16,
            })
        })
        .collect();
    [
        (values(false), Box::default()),
        (values(true), Box::default()),
        (values(true), automation),
    ]
    .into_iter()
    .filter_map(|(values, automation)| invoke(values, automation).ok())
    .collect()
}

fn ramp() -> Curve {
    Curve {
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
    }
}

fn populated_value(ty: &Type) -> Value {
    match ty {
        Type::Marks => Value::Marks(Shared::new(Marks::new(
            [250_000, 500_000, 1_250_000].map(SampleDuration::from_ticks),
        ))),
        Type::Curve => Value::Curve(Shared::new(ramp())),
        Type::Gradient => Value::Gradient(Shared::new(Gradient {
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: Color::BLACK,
                },
                GradientStop {
                    position: 1.0,
                    color: Color {
                        red: 255,
                        green: 128,
                        blue: 0,
                    },
                },
            ],
        })),
        Type::Array(item) => {
            Value::Array(Shared::from([populated_value(item), populated_value(item)]))
        }
        ty => ty.default_value(),
    }
}

fn constants() -> [ProgramConstants; 2] {
    [
        ProgramConstants::default(),
        ProgramConstants {
            pixel_count: Some(7),
            duration_seconds: Some(2.0),
        },
    ]
}

#[test]
fn deeply_nested_source_reports_a_diagnostic() {
    let nested_type = format!(
        "effect Probe {{ param values: {}int{}; sample {{ #000000 }} }}",
        "array<".repeat(512),
        ">".repeat(512)
    );
    let nested_unary = format!(
        "effect Probe {{ sample {{ rgb({}1, 0.0, 0.0) }} }}",
        "-".repeat(512)
    );
    let nested_parentheses = format!(
        "effect Probe {{ sample {{ rgb({}1{}, 0.0, 0.0) }} }}",
        "(".repeat(512),
        ")".repeat(512)
    );
    let nested_blocks = format!(
        "effect Probe {{ sample {{ {}#000000{} }} }}",
        "if pixel.fraction > 0.5 { ".repeat(512),
        " } else { #000000 }".repeat(512)
    );
    let nested_reductions = format!(
        "effect Probe {{ sample {{ {}#000000{} }} }}",
        "max for i in 0..2 { ".repeat(512),
        " }".repeat(512)
    );
    // A chain parses in a loop but nests its syntax tree.
    let long_chain = format!(
        "effect Probe {{ sample {{ rgb(pixel.fraction{}, 0.0, 0.0) }} }}",
        " + pixel.fraction".repeat(100_000)
    );

    // Rejection must fit the smallest host stack, a 1 MiB main thread.
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            for source in [
                nested_type,
                nested_unary,
                nested_parentheses,
                nested_blocks,
                nested_reductions,
                long_chain,
            ] {
                let Err(diagnostics) = compile_effects(&source) else {
                    panic!("nesting must be rejected: {}", &source[..64]);
                };
                assert!(
                    diagnostics
                        .iter()
                        .any(|diagnostic| diagnostic.message.contains("nesting")),
                    "missing nesting diagnostic: {diagnostics:?}"
                );
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
