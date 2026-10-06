use donder_language::dsl::{compile_effects, compile_operators};

#[test]
fn malformed_source_never_panics_during_compilation() {
    let templates = [
        (
            "effect Probe { param float amount in 0.0..1.0 = 0.5; color sample() { float x = amount; for (int i = 0; i < 3; i = i + 1) { x = x * 0.5; } return rgb(x, 0.0, 0.0); } }",
            true,
        ),
        (
            "operator Probe { input Signal source; param float gain in 0.0..1.0 = 0.5; color sample() { return source.at(seconds() - gain); } }",
            false,
        ),
        (
            "effect Probe { param marks beats; param int count in 0..100 = 2; color sample() { float sum = 0.0; for (int i in range(count)) { for (int mark in beats) { sum = sum + mark_at(beats, mark); } } return rgb(sum, 0.0, 0.0); } }",
            true,
        ),
    ];
    let alphabet = b"{}()[];,:.<>!=+-*/%#abc0 9\n\"";
    let mut state = 0x51a7_d5e3_u64;
    for (template, is_effect) in templates {
        for case in 0..400 {
            let mut bytes = template.as_bytes().to_vec();
            for _ in 0..(case % 5 + 1) {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let position = (state >> 32) as usize % bytes.len();
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let character = alphabet[(state >> 32) as usize % alphabet.len()];
                match state % 3 {
                    0 => bytes[position] = character,
                    1 => {
                        bytes.insert(position, character);
                    }
                    _ => {
                        bytes.remove(position);
                    }
                }
            }
            let source = String::from_utf8(bytes).unwrap();
            assert!(
                std::panic::catch_unwind(|| {
                    if is_effect {
                        let _ = compile_effects(&source);
                    } else {
                        let _ = compile_operators(&source);
                    }
                })
                .is_ok(),
                "compiler panicked for mutation {case}: {source}"
            );
        }
    }
}

#[test]
fn deeply_nested_source_reports_a_diagnostic() {
    let nested_type = format!(
        "effect Probe {{ param {}int{} values; color sample() {{ return #000000; }} }}",
        "array<".repeat(512),
        ">".repeat(512)
    );
    let nested_unary = format!(
        "effect Probe {{ color sample() {{ return {}1; }} }}",
        "-".repeat(512)
    );
    let nested_blocks = format!(
        "effect Probe {{ color sample() {{ {}return #000000; {} }} }}",
        "if (true) {".repeat(512),
        "}".repeat(512)
    );

    for source in [nested_type, nested_unary, nested_blocks] {
        let diagnostics = compile_effects(&source).expect_err("nesting must be rejected");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("syntax nesting exceeds")),
            "missing nesting diagnostic: {diagnostics:?}"
        );
    }
}
