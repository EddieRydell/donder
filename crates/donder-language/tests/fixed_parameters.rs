use donder_language::dsl::{compile_effects, compile_operators};

#[test]
fn fixed_metadata_is_shared_by_effects_and_operators() {
    let effects = compile_effects("effect Example { fixed param int count = 8; param float level = 1.0; color sample() { return rgb(level, level, level); } }").unwrap();
    assert!(effects[0].params()[0].fixed);
    assert!(!effects[0].params()[0].supports_automation());
    assert!(effects[0].params()[1].supports_automation());
    let operators = compile_operators("operator Example { input Signal source; fixed param float offset = 0.5; color sample() { return source.at(seconds() - offset); } }").unwrap();
    assert!(operators[0].params()[0].fixed);
}

#[test]
fn fixed_assignments_reject_live_values() {
    assert!(compile_effects("effect Example { fixed param float value = 0.0; color sample() { value = seconds(); return rgb(value, value, value); } }").unwrap_err().iter().any(|error| error.message.contains("fixed parameter `value`")));
}
