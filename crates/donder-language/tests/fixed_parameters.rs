use donder_language::dsl::{compile_effects, compile_operators};

#[test]
fn fixed_metadata_is_shared_by_effects_and_operators() {
    let effects = compile_effects("effect Example { fixed param int count = 8; param float level = 1.0; color sample() { return rgb(level, level, level); } }").unwrap();
    assert!(effects[0].effect.params[0].fixed);
    assert!(!effects[0].effect.params[0].supports_automation());
    assert!(effects[0].effect.params[1].supports_automation());
    let operators = compile_operators("operator Example { input Signal source; fixed param float offset = 0.5; color sample() { return source.at(seconds() - offset); } }").unwrap();
    assert!(operators[0].params[0].fixed);
}

fn generator(body: &str) -> String {
    format!(
        "effect Parent {{ fixed param int count = 4; param float live = 1.0; void generate() {{ {body} }} }}"
    )
}

const EMIT: &str =
    "timeline.emit Child { start: 0.0, duration: 1.0, target: target, level: live };";

#[test]
fn structural_dependencies_include_arrays_merges_and_loop_carried_values() {
    for body in [
        "timeline.emit Child { start: live, duration: 1.0, target: target };".to_string(),
        "array<float> values = [live]; float start = values[0]; timeline.emit Child { start: start, duration: 1.0, target: target };".to_string(),
        "float start = 0.0; if (live > 0.5) { start = 1.0; } timeline.emit Child { start: start, duration: 1.0, target: target };".to_string(),
        format!("if (live > 0.5) {{ {EMIT} }} else {{ {EMIT} }}"),
        "float start = 0.0; for (int i = 0; i < count; i = i + 1) { timeline.emit Child { start: start, duration: 1.0, target: target }; start = live; }".to_string(),
        format!("for (int i = 0; i < live; i = i + 1) {{ {EMIT} }}"),
    ] {
        let errors = compile_effects(&generator(&body)).unwrap_err();
        assert!(errors.iter().any(|error| error.message.contains("live dependency `live`") && error.message.contains("fixed")), "{errors:?}");
    }
}

#[test]
fn pure_live_calculations_and_fixed_expansion_are_allowed() {
    let body = "float level = live * 0.5; if (live > 0.5) { level = live * 0.25; } for (int i = 0; i < count; i = i + 1) { timeline.emit Child { start: i * 0.25, duration: 1.0, target: target, level: level + seconds() }; }";
    compile_effects(&generator(body)).unwrap();
}

#[test]
fn reassignment_can_remove_a_live_dependency() {
    compile_effects(&generator("float start = live; start = 0.0; timeline.emit Child { start: start, duration: 1.0, target: target };" )).unwrap();
}

#[test]
fn fixed_assignments_and_generator_pixel_reads_are_rejected() {
    assert!(compile_effects("effect Example { fixed param float value = 0.0; color sample() { value = seconds(); return rgb(value, value, value); } }").unwrap_err().iter().any(|error| error.message.contains("fixed parameter `value`")));
    assert!(
        compile_effects(&generator("float x = pixel_fraction();"))
            .unwrap_err()
            .iter()
            .any(|error| error.message.contains("pixel context"))
    );
}
