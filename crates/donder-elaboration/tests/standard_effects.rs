const SPATIAL: donder_runtime::SpatialContext = donder_runtime::SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
};

use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare};
use donder_language::dsl::{Identifier, RunContext, Value, VmWorkspace, compile_effects};
use donder_language::effect::{
    CurveSource, EffectImplementation, EffectParamValue, EffectRef, GradientSource,
};
use donder_language::values::{
    Color, Curve, CurvePoint, DonderDuration, DonderTime, Gradient, GradientStop, SampleDuration,
};
use donder_project_io::load_project;
use std::time::Duration;

#[test]
fn standard_mark_effects_prepare_real_pulse_and_chase_children_without_hue_controls() {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let source = include_str!("../../../examples/starter/effects/standard.effect.donder");
    let compiled = compile_effects(source).unwrap();
    let session = load_project(&root).unwrap();
    for (parent_name, child_name) in [("MarkPulse", "Pulse"), ("MarkChase", "Chase")] {
        let mut project = session.project.clone();
        let parent = project
            .definitions()
            .effects
            .definitions
            .iter()
            .find(|(_, definition)| definition.source_name == parent_name)
            .unwrap();
        let parent_id = parent.0.clone();
        let parent_definition = parent.1;
        assert_eq!(parent_definition.generated_effect_targets().len(), 1);
        assert!(
            !parent_definition
                .params()
                .iter()
                .any(|param| matches!(param.name.as_str(), "hue" | "hue_mix" | "base"))
        );
        let EffectRef::Custom(child_id) = &parent_definition.generated_effect_targets()[0];
        let child_definition = project.definitions().effects.get(child_id).unwrap();
        assert_eq!(child_definition.source_name, child_name);
        let EffectImplementation::Dsl(child_compiled) = child_definition.implementation();
        assert_eq!(
            child_compiled.sample_program().unwrap(),
            compiled
                .iter()
                .find(|definition| definition.effect.name().as_str() == child_name)
                .unwrap()
                .effect
                .sample_program()
                .unwrap()
        );

        let sequence_id = project
            .root()
            .sequences
            .iter()
            .map(|source| source.id())
            .find(|id| id.0.root_source().object() == "layer_test")
            .unwrap()
            .clone();
        let mut sequence = project.sequence(&sequence_id).unwrap().clone();
        sequence.mark_collections[0].marks = vec![DonderTime(Duration::from_secs(2))];
        let mut instance = sequence.effects[0].clone();
        instance.definition = EffectRef::Custom(parent_id);
        instance.layer_id = sequence.layers[0].id.clone();
        instance.start = DonderTime(Duration::ZERO);
        instance.duration = DonderDuration(Duration::from_secs(8));
        instance.param_overrides.clear();
        let add = |instance: &mut donder_language::effect::EffectInst, name: &str, value| {
            instance
                .param_overrides
                .insert(Identifier::new(name.into()).unwrap(), value);
        };
        add(
            &mut instance,
            "beats",
            EffectParamValue::Marks(sequence.mark_collections[0].key.clone()),
        );
        let falloff = EffectParamValue::Curve(CurveSource::Inline(Curve {
            points: vec![
                CurvePoint {
                    position: 0.0,
                    value: 1.0,
                },
                CurvePoint {
                    position: 1.0,
                    value: 0.0,
                },
            ],
        }));
        let position = EffectParamValue::Curve(CurveSource::Inline(Curve {
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
        }));
        let gradient = EffectParamValue::Gradient(GradientSource::Inline(Gradient {
            stops: vec![GradientStop {
                position: 0.0,
                color: Color {
                    red: 255,
                    green: 64,
                    blue: 16,
                },
            }],
        }));
        add(&mut instance, "pulse_shape", falloff);
        if parent_name == "MarkPulse" {
            add(&mut instance, "accent", gradient);
        } else {
            add(
                &mut instance,
                "gradients",
                EffectParamValue::Array(vec![gradient]),
            );
            add(
                &mut instance,
                "chase_positions",
                EffectParamValue::Array(vec![position]),
            );
        }
        sequence.effects = vec![instance];
        sequence.automation_clips.clear();
        project.replace_sequence(&sequence_id, sequence).unwrap();
        let prepared = prepare(&project, &sequence_id, PrepareOutputs::All).unwrap();
        assert!(prepared.effect_count() > 0);
    }
}

#[test]
fn standard_pulse_obeys_linear_falloff_without_an_extra_envelope() {
    let pulse = compile_effects(include_str!(
        "../../../examples/starter/effects/standard.effect.donder"
    ))
    .unwrap()
    .remove(0)
    .effect;
    let params = pulse
        .bind(
            [
                (
                    Identifier::new("gradient".into()).unwrap(),
                    Value::Gradient(
                        Gradient {
                            stops: vec![GradientStop {
                                position: 0.0,
                                color: Color {
                                    red: 255,
                                    green: 255,
                                    blue: 255,
                                },
                            }],
                        }
                        .into(),
                    ),
                ),
                (
                    Identifier::new("pulse_shape".into()).unwrap(),
                    Value::Curve(
                        Curve {
                            points: vec![
                                CurvePoint {
                                    position: 0.0,
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
                ),
            ]
            .iter()
            .map(|(name, value)| (name, value)),
            &mut donder_runtime::DslBindCache::default(),
        )
        .unwrap();
    for (progress, brightness) in [(0.0, 255), (0.25, 191), (0.5, 128), (0.75, 64), (1.0, 0)] {
        let color = params.evaluate(
            &RunContext {
                progress,
                time: SampleDuration::from_ticks((progress * 1_000_000.0) as u32),
                duration: SampleDuration::from_ticks(1_000_000),
                pixel_index: 0,
                pixel_count: 1,
                pixel_fraction: 0.0,
            },
            &SPATIAL,
            &mut VmWorkspace::default(),
        );
        assert_eq!(color.red, brightness, "progress={progress}");
        assert_eq!(color.green, brightness);
        assert_eq!(color.blue, brightness);
    }
}
