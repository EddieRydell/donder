//! Typed generator fixtures shared by host benchmarks and device archives.
use super::workload::{SampleFixture, Workload, link_generator};
use donder_language::dsl::compile_effects;
use donder_runtime::{
    AutomationMapping, BoundParams, Curve, CurvePoint, DslBindCache, GeneratorPlayback, Gradient,
    GradientStop, Identifier, PreparedAutomation, SampleDuration, SampleTime, Value,
};

#[derive(Clone, Copy, Debug)]
pub enum Case {
    Forward,
    Derived,
    Nested,
    Resources,
    Overlap,
    Curve,
}

pub const CASES: [(Case, &str); 6] = [
    (Case::Forward, "GeneratorForward"),
    (Case::Derived, "GeneratorDerived"),
    (Case::Nested, "GeneratorNested"),
    (Case::Resources, "GeneratorResources"),
    (Case::Overlap, "GeneratorOverlap"),
    (Case::Curve, "GeneratorCurve"),
];

pub fn show(count: usize, case: Case, generator: bool, automated: bool) -> Workload {
    let leaf = "effect Leaf { param float value; color sample() { return rgb(value, pixel_fraction() * value, value * 0.25); } }";
    let params = "param float level = 0.5;";
    let resources = "param array<gradient> ramps; param array<curve> shapes;";
    let selection = "int selected = 0; if (level >= 0.4) { selected = 1; }";
    let curve_sample = "color sample() { return rgb(shape[progress()], curve_crossing(shape, pixel_fraction()), 0.25); }";
    let source = if generator {
        match case {
            Case::Curve => format!(
                "effect Parent {{ {params} param curve shape; void generate() {{ timeline.emit Leaf {{ start: 0.0, duration: duration, target: target, shape: shape }}; }} }} effect Leaf {{ param curve shape; {curve_sample} }}"
            ),
            Case::Forward | Case::Derived => format!(
                "effect Parent {{ {params} void generate() {{ timeline.emit Leaf {{ start: 0.0, duration: duration, target: target, value: {} }}; }} }} {leaf}",
                if matches!(case, Case::Derived) {
                    "level * 0.5 + 0.125"
                } else {
                    "level"
                }
            ),
            Case::Nested => format!(
                "effect Parent {{ {params} void generate() {{ timeline.emit Inner {{ start: 0.0, duration: duration, target: target, value: level * 0.5 }}; }} }} effect Inner {{ param float value; void generate() {{ timeline.emit Leaf {{ start: 0.0, duration: duration, target: target, value: value + 0.125 }}; }} }} {leaf}"
            ),
            Case::Resources => format!(
                "effect Parent {{ {params} {resources} void generate() {{ {selection} timeline.emit Pulse {{ start: 0.0, duration: duration, target: target, gradient: ramps[selected], pulse_shape: shapes[selected] }}; }} }}"
            ),
            Case::Overlap => format!(
                "effect Parent {{ {params} fixed param int count = 4; void generate() {{ for (int i in range(count, 10000)) {{ timeline.emit Leaf {{ start: 0.0, duration: duration, target: target, value: level * 0.5 + i * 0.05 }}; }} }} }} {leaf}"
            ),
        }
    } else {
        match case {
            Case::Curve => {
                format!("effect Parent {{ {params} param curve shape; {curve_sample} }}")
            }
            Case::Resources => format!(
                "effect Parent {{ {params} {resources} color sample() {{ {selection} return ramps[selected][progress()] * shapes[selected][progress()]; }} }}"
            ),
            _ => format!(
                "effect Parent {{ {params} param float offset = 0.0; color sample() {{ float value = {}; return rgb(value, pixel_fraction() * value, value * 0.25); }} }}",
                match case {
                    Case::Forward => "level",
                    Case::Derived | Case::Nested => "level * 0.5 + 0.125",
                    Case::Overlap => "level * 0.5 + offset",
                    Case::Resources | Case::Curve => unreachable!(),
                }
            ),
        }
    };
    let source = format!(
        "{source}\n{}",
        include_str!("../../../examples/starter/effects/standard.effect.donder")
    );
    let definitions = compile_effects(&source).unwrap();
    let parent = &definitions
        .iter()
        .find(|item| item.effect.name().as_str() == "Parent")
        .unwrap()
        .effect;
    let mut overrides = Vec::new();
    if matches!(case, Case::Resources) {
        let ramps = (0..2)
            .map(|index| {
                Value::Gradient(
                    Gradient {
                        stops: vec![GradientStop {
                            position: 0.0,
                            color: donder_runtime::hsv(index as f32 * 0.6, 1.0, 1.0),
                        }],
                    }
                    .into(),
                )
            })
            .collect();
        let shapes = (0..2)
            .map(|index| {
                Value::Curve(
                    Curve {
                        points: vec![
                            CurvePoint {
                                position: 0.0,
                                value: index as f32,
                            },
                            CurvePoint {
                                position: 1.0,
                                value: 1.0 - index as f32,
                            },
                        ],
                    }
                    .into(),
                )
            })
            .collect();
        overrides.push((
            Identifier::new("ramps".into()).unwrap(),
            Value::Array(ramps),
        ));
        overrides.push((
            Identifier::new("shapes".into()).unwrap(),
            Value::Array(shapes),
        ));
    }
    if matches!(case, Case::Curve) {
        overrides.push((
            Identifier::new("shape".into()).unwrap(),
            Value::Curve(
                Curve {
                    points: vec![
                        CurvePoint {
                            position: 0.0,
                            value: 0.0,
                        },
                        CurvePoint {
                            position: 0.5,
                            value: 1.0,
                        },
                        CurvePoint {
                            position: 1.0,
                            value: 0.0,
                        },
                    ],
                }
                .into(),
            ),
        ));
    }
    let automation = if automated {
        let name = if matches!(case, Case::Curve) {
            "shape"
        } else {
            "level"
        };
        vec![PreparedAutomation {
            start: SampleTime::from_ticks(0),
            duration: SampleDuration::from_ticks(8_000_000),
            curve: Curve {
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
            .into(),
            mapping: if matches!(case, Case::Curve) {
                AutomationMapping::Curve { min: 0.0, max: 1.0 }
            } else {
                AutomationMapping::Float { min: 0.0, max: 1.0 }
            },
            param_index: parent
                .params()
                .iter()
                .position(|param| param.name.as_str() == name)
                .unwrap() as u16,
        }]
    } else {
        vec![]
    };
    if generator {
        let params = BoundParams::bind_pairs(parent.params(), &overrides).unwrap();
        Workload::generator(
            count,
            GeneratorPlayback::admit(
                link_generator(&definitions, "Parent"),
                params.iter_values().collect(),
                automation.into(),
                &mut DslBindCache::default(),
            )
            .unwrap(),
        )
    } else {
        let effect_count = if matches!(case, Case::Overlap) { 4 } else { 1 };
        let effects = (0..effect_count)
            .map(|index| {
                let mut overrides = overrides.clone();
                if matches!(case, Case::Overlap) {
                    overrides.push((
                        Identifier::new("offset".into()).unwrap(),
                        Value::Float(index as f32 * 0.05),
                    ));
                }
                SampleFixture {
                    program: parent.sample_program().unwrap().clone(),
                    params: BoundParams::bind_pairs(parent.params(), &overrides).unwrap(),
                    start: SampleTime::from_ticks(0),
                    duration: SampleDuration::from_ticks(8_000_000),
                    automation: automation.clone(),
                }
            })
            .collect();
        Workload::samples(count, vec![effects])
    }
}
