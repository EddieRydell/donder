use donder_language::dsl::builtins::{BUILTINS, builtin_reference};

#[test]
fn committed_builtin_reference_is_current() {
    // A Windows checkout may convert line endings.
    let committed = include_str!("../../../docs/effect_builtins.md").replace("\r\n", "\n");
    assert!(
        committed == builtin_reference(),
        "docs/effect_builtins.md is stale; run `pnpm generate:builtins`"
    );
}

#[test]
fn builtin_names_are_unique() {
    for (index, builtin) in BUILTINS.iter().enumerate() {
        assert!(
            BUILTINS[..index]
                .iter()
                .all(|other| other.name != builtin.name),
            "`{}` is listed twice",
            builtin.name
        );
    }
}
