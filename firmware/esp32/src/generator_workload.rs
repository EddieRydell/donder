//! Host-built generator fixtures, archived for the same portable device evaluator.
use dawn_language::dsl::{BoundParams, Identifier, compile_effects, validate_emission};
use dawn_language::effect::*;
use dawn_language::element::*;
use dawn_language::identity::{DocumentId, SourceIdentity};
use dawn_language::imports::SourceReference;
use dawn_language::model::*;
use dawn_language::sequence::*;
use dawn_language::setup::*;
use dawn_language::values::*;
use dawn_runtime::sequence::PreparedSequence;
use indexmap::IndexMap;
use std::time::Duration;

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

pub fn show(count: usize, case: Case, generator: bool, automated: bool) -> PreparedSequence {
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
                "effect Parent {{ {params} {resources} void generate() {{ {selection} timeline.emit builtins.pulse {{ start: 0.0, duration: duration, target: target, gradient: ramps[selected], pulse_shape: shapes[selected] }}; }} }}"
            ),
            Case::Overlap => format!(
                "effect Parent {{ {params} fixed param int count = 4; void generate() {{ for (int i = 0; i < count; i = i + 1) {{ timeline.emit Leaf {{ start: 0.0, duration: duration, target: target, value: level * 0.5 + i * 0.05 }}; }} }} }} {leaf}"
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
    let identity = |name: &str| {
        SourceIdentity::from_document(
            DocumentId::new(Default::default(), "fixture.dawn".into()),
            name.into(),
        )
    };
    let setup_id = SetupId(identity("setup"));
    let tree_id = ElementTreeId(identity("elements"));
    let sequence_id = SequenceId(identity("sequence"));
    let mut definitions = ProjectDefinitionStores::default();
    for compilation in compile_effects(&source).unwrap() {
        let id = EffectDefinitionId(identity(compilation.effect.name.as_str()));
        definitions
            .effects
            .insert(id.clone(), EffectDefinition::custom(id, compilation));
    }
    let linked = definitions
        .effects
        .definitions
        .iter()
        .map(|(id, definition)| {
            let references = definition
                .emitted_references
                .iter()
                .map(|emission| {
                    let reference = match &emission.reference {
                        SourceReference::Local(name) => {
                            EffectRef::Custom(EffectDefinitionId(identity(name.as_str())))
                        }
                        SourceReference::Builtin(name) => EffectRef::Builtin(
                            builtin_effect_from_source_name(name.as_str()).unwrap(),
                        ),
                        SourceReference::Qualified { .. } => {
                            panic!("fixture children are local or built-in")
                        }
                    };
                    validate_emission(
                        emission,
                        &definitions.effects.resolve(&reference).unwrap().params,
                    )
                    .unwrap();
                    reference
                })
                .collect::<Box<[_]>>();
            (id.clone(), references)
        })
        .collect::<Vec<_>>();
    for (id, references) in linked {
        definitions
            .effects
            .definitions
            .get_mut(&id)
            .unwrap()
            .generated_effect_targets = references;
    }
    let effect_count = if !generator && matches!(case, Case::Overlap) {
        4
    } else {
        1
    };
    let mut sequence = Sequence {
        id: sequence_id.clone(),
        duration: DawnDuration(Duration::from_secs(8)),
        frame_rate: 120,
        audio: SequenceAudio::None,
        mark_collections: vec![],
        control_clips: vec![],
        layers: vec![SequenceLayer {
            id: SequenceLayerId(0),
            name: "fixture".into(),
            color: dawn_runtime::element::black(),
            enabled: true,
        }],
        effects: (0..effect_count)
            .map(|index| EffectInst {
                id: EffectInstId(index),
                layer_id: SequenceLayerId(0),
                start: DawnTime(Duration::ZERO),
                duration: DawnDuration(Duration::from_secs(8)),
                target: ElementSelection {
                    tree: tree_id.clone(),
                    node: ElementNodeId(0),
                    cells: None,
                },
                scope: EffectScope::WholeTarget,
                definition: EffectRef::Custom(EffectDefinitionId(identity("Parent"))),
                param_overrides: if !generator && matches!(case, Case::Overlap) {
                    [(
                        Identifier::new("offset".into()).unwrap(),
                        EffectParamValue::Float(index as f32 * 0.05),
                    )]
                    .into()
                } else {
                    IndexMap::new()
                },
            })
            .collect(),
        automation_clips: vec![],
        composition_graph: SequenceCompositionGraph {
            nodes: vec![
                CompositionGraphNode {
                    id: CompositionGraphNodeId(0),
                    position: GraphNodePosition { x: 0.0, y: 0.0 },
                    kind: CompositionGraphNodeKind::Layer {
                        layer_id: SequenceLayerId(0),
                    },
                },
                CompositionGraphNode {
                    id: CompositionGraphNodeId(1),
                    position: GraphNodePosition { x: 1.0, y: 0.0 },
                    kind: CompositionGraphNodeKind::Output,
                },
            ],
            edges: vec![EffectGraphEdge {
                from: CompositionGraphNodeId(0),
                from_port: GraphPortId("output".into()),
                to: CompositionGraphNodeId(1),
                to_port: GraphPortId("input".into()),
            }],
        },
    };
    if matches!(case, Case::Resources) {
        for effect in &mut sequence.effects {
            let ramps = (0..2)
                .map(|index| {
                    EffectParamValue::Gradient(GradientSource::Inline(Gradient {
                        stops: vec![GradientStop {
                            position: 0.0,
                            color: dawn_runtime::sampling::hsv(index as f32 * 0.6, 1.0, 1.0),
                        }],
                    }))
                })
                .collect();
            let shapes = (0..2)
                .map(|index| {
                    EffectParamValue::Curve(CurveSource::Inline(Curve {
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
                    }))
                })
                .collect();
            effect.param_overrides.insert(
                Identifier::new("ramps".into()).unwrap(),
                EffectParamValue::Array(ramps),
            );
            effect.param_overrides.insert(
                Identifier::new("shapes".into()).unwrap(),
                EffectParamValue::Array(shapes),
            );
        }
    }
    if matches!(case, Case::Curve) {
        sequence.effects[0].param_overrides.insert(
            Identifier::new("shape".into()).unwrap(),
            EffectParamValue::Curve(CurveSource::Inline(Curve {
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
            })),
        );
    }
    if automated {
        sequence.automation_clips.push(AutomationClip {
            id: AutomationClipId(0),
            start: DawnTime(Duration::ZERO),
            duration: DawnDuration(Duration::from_secs(8)),
            anchor_lane_index: 0,
            lane_index: 0,
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
            },
            bindings: sequence
                .effects
                .iter()
                .map(|effect| AutomationBinding {
                    target: AutomationTarget::EffectParam {
                        effect_id: effect.id.clone(),
                        param: Identifier::new(
                            if matches!(case, Case::Curve) {
                                "shape"
                            } else {
                                "level"
                            }
                            .into(),
                        )
                        .unwrap(),
                    },
                    mapping: if matches!(case, Case::Curve) {
                        AutomationMapping::Curve { min: 0.0, max: 1.0 }
                    } else {
                        AutomationMapping::Float { min: 0.0, max: 1.0 }
                    },
                })
                .collect(),
            detached_bindings: vec![],
        });
    }
    let project = DawnProject {
        root: ProjectRoot {
            id: ProjectId(identity("project")),
            setup: setup_id.clone(),
            sequences: vec![sequence_id.clone()],
        },
        setups: [(
            setup_id.clone(),
            Setup {
                id: setup_id.clone(),
                elements: tree_id.clone(),
                preview: dawn_language::preview::PreviewLayoutId(identity("preview")),
                patch: dawn_language::patch::PatchId(identity("patch")),
                controllers: vec![],
            },
        )]
        .into(),
        element_trees: [(
            tree_id.clone(),
            ElementTree {
                id: tree_id,
                roots: vec![ElementNodeId(0)],
                nodes: [(
                    ElementNodeId(0),
                    ElementNode {
                        name: "pixels".into(),
                        kind: ElementNodeKind::Color {
                            cells: count as u32,
                            capability: ColorCapability::Rgb,
                        },
                    },
                )]
                .into(),
            },
        )]
        .into(),
        preview_layouts: IndexMap::new(),
        patches: IndexMap::new(),
        controllers: IndexMap::new(),
        sequences: [(sequence_id.clone(), sequence)].into(),
        definitions,
    };
    let signals = dawn_elaboration::elaborate_sequence(&project, &setup_id, &sequence_id).unwrap();
    let dummy =
        compile_effects("effect Placeholder { color sample() { return rgb(0.0, 0.0, 0.0); } }")
            .unwrap()
            .remove(0)
            .effect
            .bytecode;
    let mut output = super::workload::show(count, dummy, BoundParams::default());
    output.signals = signals;
    output
}
