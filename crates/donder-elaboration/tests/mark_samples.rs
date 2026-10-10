use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare};
use donder_language::{DonderDuration, DonderTime};
use donder_model::DonderProject;
use donder_model::SequenceId;
use donder_model::{CurveSource, EffectParamValue, EffectRef, EffectScope, GradientSource};
use donder_project_io::load_project;
use donder_runtime::PreparedSequence;
use donder_runtime::SequencePlayback;
use donder_runtime_types::Identifier;
use donder_runtime_types::SampleTime;
use donder_runtime_types::{Color, Curve, CurvePoint, Gradient, GradientStop};
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
    marks_ms: &[u64],
    scope: EffectScope,
) -> PreparedSequence {
    prepared_with_chase_position(project, effect, marks_ms, scope, shape(0.0, 1.0))
}

fn prepared_with_chase_position(
    project: &DonderProject,
    effect: MarkEffect,
    marks_ms: &[u64],
    scope: EffectScope,
    chase_position: EffectParamValue,
) -> PreparedSequence {
    let (project, id) = configured_project(project, effect, marks_ms, scope, chase_position);
    prepare(&project, &id, PrepareOutputs::All).unwrap()
}

fn configured_project(
    project: &DonderProject,
    effect: MarkEffect,
    marks_ms: &[u64],
    scope: EffectScope,
    chase_position: EffectParamValue,
) -> (DonderProject, SequenceId) {
    let mut project = project.clone();
    let name = effect.name();
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
    let mut instance = (*sequence.effects[0]).clone();
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
        EffectParamValue::Marks(Some(sequence.mark_collections[0].key.clone())),
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
    // A nonzero endpoint distinguishes explicit pulse lifetime from endpoint holding.
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
            set("chase_position", chase_position);
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
    sequence.effects = vec![std::sync::Arc::new(instance)];
    let id = sequence.id.clone();
    project.replace_sequence(&id, sequence).unwrap();
    (project, id)
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

// Independent reference: one ordinary effect placed at the mark's arrival time.
// No mark query or mark-effect implementation is involved in this playback.
fn single_pass_reference(
    project: &DonderProject,
    effect: MarkEffect,
    mark_ms: u64,
    scope: EffectScope,
    trajectory: EffectParamValue,
) -> PreparedSequence {
    let (mut project, id) = configured_project(project, effect, &[mark_ms], scope, trajectory);
    let name = match effect {
        MarkEffect::Chase => "Chase",
        MarkEffect::Wipe => "Wipe",
        MarkEffect::ImpactBurst => "ImpactBurst",
        MarkEffect::Pulse => "Pulse",
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
    let mut sequence = project.sequence(&id).unwrap().clone();
    let instance = std::sync::Arc::make_mut(&mut sequence.effects[0]);
    instance.definition = EffectRef::Custom(definition);
    instance.start = DonderTime(Duration::from_millis(mark_ms + 125));
    instance.duration = DonderDuration(Duration::from_secs(1));
    let params = &mut instance.param_overrides;
    for name in ["beats", "offset_seconds", effect.duration_param()] {
        params.shift_remove(name).unwrap();
    }
    let gradient = if matches!(effect, MarkEffect::Pulse) {
        for name in ["section_width_pixels", "sections_per_mark", "seed"] {
            params.shift_remove(name).unwrap();
        }
        params.shift_remove("accent").unwrap()
    } else {
        let EffectParamValue::Array(mut gradients) = params.shift_remove("gradients").unwrap()
        else {
            panic!("gradient fixture must be an array");
        };
        gradients.remove(0)
    };
    params.insert(Identifier::new("gradient".into()).unwrap(), gradient);
    if matches!(effect, MarkEffect::Wipe) {
        let EffectParamValue::Array(mut positions) = params.shift_remove("wipe_positions").unwrap()
        else {
            panic!("wipe fixture must be an array");
        };
        params.insert(
            Identifier::new("wipe_position".into()).unwrap(),
            positions.remove(0),
        );
        params.insert(
            Identifier::new("direction_angle".into()).unwrap(),
            // MarkWipe authors turns; the ordinary Wipe parameter uses degrees.
            EffectParamValue::Float(45.0),
        );
    }
    project.replace_sequence(&id, sequence).unwrap();
    prepare(&project, &id, PrepareOutputs::All).unwrap()
}

// MarkPulse chooses fixture-aligned sections, including the short final section.
// Mask ordinary Pulse output with the original seeded section selection rule.
fn selected_sections(sequence: &PreparedSequence, scope: &EffectScope) -> Vec<bool> {
    let total = sequence
        .fixtures()
        .iter()
        .map(|fixture| fixture.pixel_count.div_ceil(7))
        .sum::<usize>();
    let mut base = 0;
    sequence
        .fixtures()
        .iter()
        .flat_map(|fixture| {
            let count = match scope {
                EffectScope::WholeTarget => total,
                EffectScope::PerFixture => fixture.pixel_count.div_ceil(7),
            };
            let choices = [0.0, 1.0, 2.0].map(|choice| {
                (donder_runtime_types::sampling::deterministic_random(
                    [1000.75, choice].into_iter(),
                ) * count as f32)
                    .floor() as usize
            });
            let start = match scope {
                EffectScope::WholeTarget => base,
                EffectScope::PerFixture => 0,
            };
            base += fixture.pixel_count.div_ceil(7);
            (0..fixture.pixel_count).map(move |pixel| choices.contains(&(start + pixel / 7)))
        })
        .collect()
}

fn check_single_mark_lifetime(effect: MarkEffect) {
    let project = starter();
    for scope in [EffectScope::PerFixture, EffectScope::WholeTarget] {
        let mut playback = prepared(&project, effect, &[1000], scope.clone()).into_playback();
        let reference =
            single_pass_reference(&project, effect, 1000, scope.clone(), shape(0.0, 1.0));
        let mask =
            matches!(effect, MarkEffect::Pulse).then(|| selected_sections(&reference, &scope));
        let mut reference = reference.into_playback();
        let mut lit = false;
        for time in [0, 1124, 1125, 1250, 1500, 1750, 2000, 2125, 2500, 1250] {
            let actual = colors(&mut playback, time);
            let mut expected = colors(&mut reference, time);
            if let Some(mask) = &mask {
                for (color, selected) in expected.iter_mut().zip(mask) {
                    if !selected {
                        *color = Color::BLACK;
                    }
                }
            }
            assert_same_colors(
                &actual,
                &expected,
                &format!("{effect:?} {scope:?} at {time}ms"),
            );
            if !(1125..2125).contains(&time) {
                assert!(
                    actual.iter().all(|color| *color == Color::BLACK),
                    "{effect:?} must be dark outside its pulse lifetime"
                );
            } else {
                lit |= actual.iter().any(|color| *color != Color::BLACK);
            }
        }
        assert!(lit, "{effect:?} fixture must contain visible output");
    }
}

fn compare_retrigger(effect: MarkEffect) {
    let project = starter();
    let scope = EffectScope::WholeTarget;
    let mut sampled = prepared(&project, effect, &[1000, 1250], scope.clone()).into_playback();
    let mut latest_only = prepared(&project, effect, &[1250], scope.clone()).into_playback();
    let mut old_only = prepared(&project, effect, &[1000], scope).into_playback();
    let mut discarded_visible_old_pulse = false;
    for time in [1375, 1500, 1625, 1750, 2000, 2375, 2500, 1500] {
        let actual = colors(&mut sampled, time);
        assert_same_colors(
            &actual,
            &colors(&mut latest_only, time),
            &format!("{effect:?} latest mark at {time}ms"),
        );
        discarded_visible_old_pulse |= actual != colors(&mut old_only, time);
        if time >= 2375 {
            assert!(
                actual.iter().all(|color| *color == Color::BLACK),
                "{effect:?} remains lit after the last pulse ends"
            );
        }
    }
    assert!(
        discarded_visible_old_pulse,
        "{effect:?} fixture must distinguish the latest mark from the older pulse"
    );
}

#[test]
fn chase_obeys_mark_offset_and_pulse_lifetime() {
    check_single_mark_lifetime(MarkEffect::Chase);
}

#[test]
fn chase_retriggers_each_pixel_only_when_the_next_chase_arrives() {
    let project = starter();
    for scope in [EffectScope::WholeTarget, EffectScope::PerFixture] {
        for trajectory in [shape(0.0, 1.0), shape(1.0, 0.0)] {
            let prepare_chase = |marks| {
                prepared_with_chase_position(
                    &project,
                    MarkEffect::Chase,
                    marks,
                    scope.clone(),
                    trajectory.clone(),
                )
                .into_playback()
            };
            let mut sampled = prepare_chase(&[1000, 1250]);
            let mut old = single_pass_reference(
                &project,
                MarkEffect::Chase,
                1000,
                scope.clone(),
                trajectory.clone(),
            )
            .into_playback();
            let mut new = single_pass_reference(
                &project,
                MarkEffect::Chase,
                1250,
                scope.clone(),
                trajectory.clone(),
            )
            .into_playback();
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
fn wipe_obeys_lifetime_and_retriggers_without_overlap() {
    check_single_mark_lifetime(MarkEffect::Wipe);
    compare_retrigger(MarkEffect::Wipe);
}

#[test]
fn burst_obeys_lifetime_and_retriggers_without_overlap() {
    check_single_mark_lifetime(MarkEffect::ImpactBurst);
    compare_retrigger(MarkEffect::ImpactBurst);
}

#[test]
fn pulse_obeys_lifetime_and_retriggers_without_overlap() {
    check_single_mark_lifetime(MarkEffect::Pulse);
    compare_retrigger(MarkEffect::Pulse);
}
