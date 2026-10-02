use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare};
use donder_language::dsl::Identifier;
use donder_language::effect::{
    CurveSource, EffectParamValue, EffectRef, EffectScope, GradientSource,
};
use donder_language::model::DonderProject;
use donder_language::values::{
    Color, Curve, CurvePoint, DonderDuration, DonderTime, Gradient, GradientStop,
};
use donder_project_io::load_project;
use donder_runtime::{PreparedSequence, SampleTime, SequencePlayback};
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
enum MarkEffect {
    Chase,
    Wipe,
    ImpactBurst,
    Pulse,
}

impl MarkEffect {
    fn name(self) -> &'static str {
        match self {
            Self::Chase => "MarkChase",
            Self::Wipe => "MarkWipe",
            Self::ImpactBurst => "MarkImpactBurst",
            Self::Pulse => "MarkPulse",
        }
    }

    fn duration_param(self) -> &'static str {
        match self {
            Self::Chase => "chase_seconds",
            Self::Wipe => "wipe_seconds",
            Self::ImpactBurst => "burst_seconds",
            Self::Pulse => "decay_seconds",
        }
    }
}

fn starter() -> DonderProject {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    load_project(&root).unwrap().project
}

fn shape(start: f32, end: f32) -> EffectParamValue {
    EffectParamValue::Curve(CurveSource::Inline(Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: start,
            },
            CurvePoint {
                position: 1.0,
                value: end,
            },
        ],
    }))
}

fn prepared(
    project: &DonderProject,
    effect: MarkEffect,
    sampled: bool,
    marks_ms: &[u64],
    scope: EffectScope,
) -> PreparedSequence {
    prepared_with_chase_position(project, effect, sampled, marks_ms, scope, shape(0.0, 1.0))
}

fn prepared_with_chase_position(
    project: &DonderProject,
    effect: MarkEffect,
    sampled: bool,
    marks_ms: &[u64],
    scope: EffectScope,
    chase_position: EffectParamValue,
) -> PreparedSequence {
    let mut project = project.clone();
    let name = if sampled {
        format!("{}Sample", effect.name())
    } else {
        effect.name().into()
    };
    let definition = project
        .definitions()
        .effects
        .definitions
        .iter()
        .find(|(_, definition)| definition.source_name == name)
        .unwrap()
        .0
        .clone();
    let mut sequence = project
        .sequences()
        .find(|sequence| sequence.id.0.root_source().object() == "layer_test")
        .unwrap()
        .clone();
    sequence.automation_clips.clear();
    sequence.mark_collections[0].marks = marks_ms
        .iter()
        .map(|&ms| DonderTime(Duration::from_millis(ms)))
        .collect();
    let mut instance = sequence.effects[0].clone();
    instance.layer_id = sequence.layers[0].id.clone();
    instance.definition = EffectRef::Custom(definition);
    instance.scope = scope;
    instance.start = DonderTime(Duration::ZERO);
    instance.duration = DonderDuration(Duration::from_secs(4));
    instance.param_overrides.clear();
    let mut set = |name: &str, value| {
        instance
            .param_overrides
            .insert(Identifier::new(name.into()).unwrap(), value);
    };
    set(
        "beats",
        EffectParamValue::Marks(sequence.mark_collections[0].key.clone()),
    );
    set("offset_seconds", EffectParamValue::Float(0.125));
    set(effect.duration_param(), EffectParamValue::Float(1.0));
    let accent = EffectParamValue::Gradient(GradientSource::Inline(Gradient {
        stops: vec![GradientStop {
            position: 0.0,
            color: Color {
                red: 255,
                green: 128,
                blue: 64,
            },
        }],
    }));
    if matches!(effect, MarkEffect::Pulse) {
        set("accent", accent);
        // 113 pixels per fixture deliberately leaves a partial final section.
        set("section_width_pixels", EffectParamValue::Int(7));
        set("sections_per_mark", EffectParamValue::Int(3));
        set("seed", EffectParamValue::Float(0.75));
    } else {
        set("gradients", EffectParamValue::Array(vec![accent]));
    }
    // A nonzero endpoint distinguishes explicit child lifetime from endpoint holding.
    set(
        if matches!(effect, MarkEffect::ImpactBurst) {
            "intensity"
        } else {
            "pulse_shape"
        },
        shape(1.0, 0.25),
    );
    match effect {
        MarkEffect::Chase => {
            if sampled {
                set("chase_position", chase_position);
            } else {
                set(
                    "chase_positions",
                    EffectParamValue::Array(vec![chase_position]),
                );
            }
            set("section_width_pixels", EffectParamValue::Int(7));
        }
        MarkEffect::Wipe => {
            set(
                "wipe_positions",
                EffectParamValue::Array(vec![shape(0.0, 1.0)]),
            );
            set("direction_angle", shape(0.125, 0.125));
            set("pulse_width", EffectParamValue::Float(0.25));
        }
        MarkEffect::ImpactBurst => {
            set("glow_level", EffectParamValue::Float(0.0));
        }
        MarkEffect::Pulse => {}
    }
    sequence.effects = vec![instance];
    let id = sequence.id.clone();
    project.replace_sequence(&id, sequence).unwrap();
    prepare(&project, &id, PrepareOutputs::All).unwrap()
}

fn colors(playback: &mut SequencePlayback, milliseconds: u32) -> Vec<Color> {
    playback
        .evaluate(SampleTime::from_ticks(milliseconds * 1000))
        .colors()
        .to_vec()
}

fn assert_same_colors(actual: &[Color], expected: &[Color], context: &str) {
    assert_eq!(actual.len(), expected.len(), "{context}: pixel count");
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(actual, expected, "{context}: pixel {index}");
    }
}

fn compare_single_mark(effect: MarkEffect) {
    let project = starter();
    for scope in [EffectScope::PerFixture, EffectScope::WholeTarget] {
        let mut generated =
            prepared(&project, effect, false, &[1000], scope.clone()).into_playback();
        let mut sampled = prepared(&project, effect, true, &[1000], scope.clone()).into_playback();
        let mut lit = false;
        // Seek backward as well, so a cached later query cannot leak into an earlier frame.
        for time in [0, 1124, 1125, 1250, 1500, 1750, 2000, 2125, 2500, 1250] {
            let expected = colors(&mut generated, time);
            let actual = colors(&mut sampled, time);
            assert_same_colors(
                &actual,
                &expected,
                &format!("{effect:?} {scope:?} at {time}ms"),
            );
            if !(1125..2125).contains(&time) {
                assert!(
                    actual.iter().all(|color| *color == Color::BLACK),
                    "{effect:?} must be dark outside its child lifetime"
                );
            } else {
                lit |= actual.iter().any(|color| *color != Color::BLACK);
            }
        }
        assert!(lit, "{effect:?} comparison must contain visible output");
    }
}

fn compare_retrigger(effect: MarkEffect) {
    let project = starter();
    let scope = EffectScope::WholeTarget;
    let mut sampled =
        prepared(&project, effect, true, &[1000, 1250], scope.clone()).into_playback();
    let mut latest_only = prepared(&project, effect, false, &[1250], scope.clone()).into_playback();
    let mut overlapping = prepared(&project, effect, false, &[1000, 1250], scope).into_playback();
    let mut discarded_visible_old_pulse = false;
    for time in [1375, 1500, 1625, 1750, 2000, 2375, 2500, 1500] {
        let actual = colors(&mut sampled, time);
        assert_same_colors(
            &actual,
            &colors(&mut latest_only, time),
            &format!("{effect:?} latest mark at {time}ms"),
        );
        discarded_visible_old_pulse |= actual != colors(&mut overlapping, time);
        if time >= 2375 {
            assert!(
                actual.iter().all(|color| *color == Color::BLACK),
                "{effect:?} remains lit after the last child ends"
            );
        }
    }
    assert!(
        discarded_visible_old_pulse,
        "{effect:?} fixture must distinguish retrigger from overlapping generation"
    );
}

#[test]
fn chase_sample_matches_one_generated_chase() {
    compare_single_mark(MarkEffect::Chase);
}

#[test]
fn chase_sample_retriggers_each_pixel_only_when_the_next_chase_arrives() {
    let project = starter();
    for scope in [EffectScope::WholeTarget, EffectScope::PerFixture] {
        for trajectory in [shape(0.0, 1.0), shape(1.0, 0.0)] {
            let prepare_chase = |sampled, marks| {
                prepared_with_chase_position(
                    &project,
                    MarkEffect::Chase,
                    sampled,
                    marks,
                    scope.clone(),
                    trajectory.clone(),
                )
                .into_playback()
            };
            let mut sampled = prepare_chase(true, &[1000, 1250]);
            let mut old = prepare_chase(false, &[1000]);
            let mut new = prepare_chase(false, &[1250]);
            let mut preserved_old_pulse = false;
            for time in [1375, 1500, 1625, 1750, 2000, 2375, 2500, 1500] {
                let old = colors(&mut old, time);
                let new = colors(&mut new, time);
                let expected: Vec<_> = old
                    .iter()
                    .zip(&new)
                    .map(|(&old, &new)| if new != Color::BLACK { new } else { old })
                    .collect();
                preserved_old_pulse |= expected != new;
                assert_same_colors(
                    &colors(&mut sampled, time),
                    &expected,
                    &format!("{scope:?} latest arrival at {time}ms"),
                );
            }
            assert!(preserved_old_pulse);
        }
    }
}

#[test]
fn wipe_sample_matches_one_generated_wipe_and_retriggers_without_overlap() {
    compare_single_mark(MarkEffect::Wipe);
    compare_retrigger(MarkEffect::Wipe);
}

#[test]
fn burst_sample_matches_one_generated_burst_and_retriggers_without_overlap() {
    compare_single_mark(MarkEffect::ImpactBurst);
    compare_retrigger(MarkEffect::ImpactBurst);
}

#[test]
fn pulse_sample_preserves_fixture_sections_and_retriggers_without_overlap() {
    compare_single_mark(MarkEffect::Pulse);
    compare_retrigger(MarkEffect::Pulse);
}
