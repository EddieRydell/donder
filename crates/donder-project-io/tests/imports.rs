use camino::Utf8PathBuf;
use donder_language::effect::EffectRef;
use donder_language::identity::DocumentId;
use donder_language::imports::ImportAlias;
use donder_project_io::{
    SourceObjectKind, check_package_with_overrides, ensure_document_can_reference_source,
    project_source_texts,
};

fn root() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter")
}

const GENERATOR: &str = "effects/mark-impact-burst.effect.donder";
const CHILD: &str = "effects/impact-burst.effect.donder";
const EXTRA: &str = "effects/import-test.effect.donder";

#[test]
fn unused_imported_emissions_validate_types_and_fixed_parameters() {
    for (declaration, argument, expected) in [
        (
            "fixed param float value = 0.0;",
            "live",
            "fixed child parameter `value`",
        ),
        ("param bool value = false;", "live", "expects Bool"),
        ("param float value = 0.0;", "live", ""),
    ] {
        let mut sources = project_source_texts(&root()).unwrap();
        sources.insert(
            EXTRA.into(),
            format!(
                "effect Child {{ {declaration} color sample() {{ return hsv(0.0, 1.0, 1.0); }} }}"
            ),
        );
        let generator = sources.get_mut(&Utf8PathBuf::from(GENERATOR)).unwrap();
        *generator = format!(
            "import check from [\"{EXTRA}\"];\n{generator}\neffect UnusedParent {{ param float live = 0.5; void generate() {{ if (false) {{ timeline.emit check.Child {{ start: 0.0, duration: 1.0, target: target, value: {argument} }}; }} }} }}"
        );
        let report = check_package_with_overrides(&root(), &sources);
        if expected.is_empty() {
            assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        } else {
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains(expected)),
                "{:?}",
                report.diagnostics
            );
        }
    }
}

fn generator(reference: &str) -> String {
    let curve_field = if reference == "builtins.pulse" {
        "pulse_shape"
    } else {
        "intensity"
    };
    format!(
        "effect MarkImpactBurst {{ param gradient ramp; param curve shape; void generate() {{ timeline.emit {reference} {{ start: 0.0, duration: 0.1, target: target, gradient: ramp, {curve_field}: shape }}; }} }}"
    )
}

#[test]
fn yaml_and_dsl_grouped_declarations_have_identical_ordered_targets() {
    for alias in [
        "Fx",
        "_fx2",
        "an_alias_longer_than_thirty_two_bytes_is_valid",
    ] {
        let mut sources = project_source_texts(&root()).unwrap();
        sources.insert(
            GENERATOR.into(),
            format!(
                "import {alias} from [\"{CHILD}\", \"{EXTRA}\"];\n{}",
                generator(&format!("{alias}.ImpactBurst"))
            ),
        );
        sources.insert(
            EXTRA.into(),
            "effect Extra { color sample() { return hsv(0.0, 1.0, 1.0); } }".into(),
        );
        let project = sources
            .get_mut(&Utf8PathBuf::from("project.donder"))
            .unwrap();
        *project = project.replacen(
            "imports:\n",
            &format!("imports:\n- from: {{ documents: [{CHILD}, {EXTRA}] }}\n  as: {alias}\n"),
            1,
        );
        let report = check_package_with_overrides(&root(), &sources);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let session = report.session.unwrap();
        let module = session.source.project_module_id();
        let yaml = &session.source.documents[&DocumentId::new(module, "project.donder".into())]
            .imports()[0];
        let dsl =
            &session.source.documents[&DocumentId::new(module, GENERATOR.into())].imports()[0];
        assert_eq!(yaml.declaration(), dsl.declaration());
        assert_eq!(yaml.targets(), dsl.targets());
        assert_eq!(
            dsl.targets()
                .iter()
                .map(|id| id.path().as_str())
                .collect::<Vec<_>>(),
            [CHILD, EXTRA]
        );
        let definition = session
            .project
            .definitions
            .effects
            .definitions
            .values()
            .find(|definition| definition.source_name == "MarkImpactBurst")
            .unwrap();
        assert!(
            matches!(&definition.generated_effect_targets[0], EffectRef::Custom(id) if id.0.document() == CHILD)
        );
    }
}

#[test]
fn alias_policy_is_shared_without_package_name_restrictions() {
    for alias in [
        "builtins",
        "effect",
        "from",
        "if",
        "1fx",
        "with-hyphen",
        "é",
        "",
    ] {
        assert!(ImportAlias::new(alias).is_err(), "{alias}");
        let mut sources = project_source_texts(&root()).unwrap();
        sources.insert(
            GENERATOR.into(),
            format!(
                "import {alias} from [\"{CHILD}\"];\n{}",
                generator("builtins.pulse")
            ),
        );
        assert!(
            check_package_with_overrides(&root(), &sources)
                .session
                .is_none(),
            "DSL {alias}"
        );
        sources.insert(GENERATOR.into(), generator("builtins.pulse"));
        let project = sources
            .get_mut(&Utf8PathBuf::from("project.donder"))
            .unwrap();
        *project = project.replacen(
            "imports:\n",
            &format!("imports:\n- from: {{ documents: [{CHILD}] }}\n  as: '{alias}'\n"),
            1,
        );
        assert!(
            check_package_with_overrides(&root(), &sources)
                .session
                .is_none(),
            "YAML {alias}"
        );
    }
}

#[test]
fn grouped_collisions_report_both_source_occurrences_in_both_formats() {
    for yaml in [false, true] {
        for (documents, second, message) in [
            (format!("{CHILD}, {CHILD}"), None, "imported more than once"),
            (
                CHILD.into(),
                Some(("other", CHILD)),
                "imported more than once",
            ),
            (CHILD.into(), Some(("fx", EXTRA)), "duplicate import alias"),
            (
                format!("{CHILD}, {EXTRA}"),
                None,
                "duplicate exported object",
            ),
        ] {
            let mut sources = project_source_texts(&root()).unwrap();
            sources.insert(
                EXTRA.into(),
                "effect ImpactBurst { color sample() { return hsv(0.0, 1.0, 1.0); } }".into(),
            );
            let path = if yaml { "project.donder" } else { GENERATOR };
            let declaration = if yaml {
                let mut text = format!("- from: {{ documents: [{documents}] }}\n  as: fx\n");
                if let Some((alias, path)) = second {
                    text += &format!("- from: {{ documents: [{path}] }}\n  as: {alias}\n");
                }
                let original = sources[&Utf8PathBuf::from(path)].clone();
                original.replacen("imports:\n", &format!("imports:\n{text}"), 1)
            } else {
                let paths = documents
                    .split(", ")
                    .map(|path| format!("\"{path}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut text = format!("import fx from [{paths}];\n");
                if let Some((alias, path)) = second {
                    text += &format!("import {alias} from [\"{path}\"];\n");
                }
                text + &generator("fx.ImpactBurst")
            };
            sources.insert(path.into(), declaration);
            let report = check_package_with_overrides(&root(), &sources);
            let diagnostic = report
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.message.contains(message))
                .unwrap_or_else(|| panic!("{:?}", report.diagnostics));
            assert_eq!(diagnostic.path, path);
            assert!(diagnostic.range.is_some());
            assert_eq!(diagnostic.related.len(), 1);
            assert!(diagnostic.related[0].range.is_some());
            assert_ne!(diagnostic.range, diagnostic.related[0].range);
        }
    }
}

#[test]
fn missing_group_member_points_at_its_own_path_token() {
    let mut sources = project_source_texts(&root()).unwrap();
    let text = format!(
        "import fx from [\"{CHILD}\",\n  \"effects/missing.effect.donder\"];\n{}",
        generator("fx.ImpactBurst")
    );
    sources.insert(GENERATOR.into(), text);
    let report = check_package_with_overrides(&root(), &sources);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("target does not exist"))
        .unwrap();
    let range = diagnostic.range.as_ref().unwrap();
    assert_eq!((range.start.line, range.start.character), (1, 2));
    assert_eq!(range.end.character, 33);
}

#[test]
fn scope_is_non_transitive_and_wrong_kinds_and_builtin_names_are_link_errors() {
    for (imports, reference) in [
        (format!("import fx from [\"{CHILD}\"];"), "fx.Extra"),
        (
            "import fx from [\"curves/basic_curves.curve.donder\"];".into(),
            "fx.ease_down",
        ),
        (String::new(), "builtins.missing"),
        (String::new(), "missing.Child"),
    ] {
        let mut sources = project_source_texts(&root()).unwrap();
        let child = sources[&Utf8PathBuf::from(CHILD)].clone();
        sources.insert(
            CHILD.into(),
            format!("import nested from [\"{EXTRA}\"];\n{child}"),
        );
        sources.insert(
            EXTRA.into(),
            "effect Extra { color sample() { return hsv(0.0, 1.0, 1.0); } }".into(),
        );
        let text = format!("{imports}\n{}", generator(reference));
        let start = text.find(&format!("emit {reference}")).unwrap() + 5;
        let line_start = text[..start].rfind('\n').unwrap() + 1;
        sources.insert(GENERATOR.into(), text);
        let report = check_package_with_overrides(&root(), &sources);
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.path == GENERATOR
                    && diagnostic.message.contains("generated child reference")
            })
            .unwrap_or_else(|| panic!("{:?}", report.diagnostics));
        let range = diagnostic.range.as_ref().unwrap();
        assert_eq!(
            (range.start.line, range.start.character),
            (1, (start - line_start) as u32)
        );
        assert_eq!(
            range.end.character - range.start.character,
            reference.len() as u32
        );
    }
}

#[test]
fn local_imports_and_package_exports_share_safe_path_policy() {
    for path in [
        "../escape.donder",
        "/root.donder",
        "effects//child.donder",
        "./child.donder",
        "effects/../child.donder",
        "CON.donder",
        "effects./child.donder",
        "effects /child.donder",
        "child.txt",
        "C:/child.donder",
        "effects\\child.donder",
        "",
    ] {
        assert!(
            donder_package::validate_module_relative_donder_path(path).is_err(),
            "{path}"
        );
        for yaml in [false, true] {
            let mut sources = project_source_texts(&root()).unwrap();
            if yaml {
                let project = sources
                    .get_mut(&Utf8PathBuf::from("project.donder"))
                    .unwrap();
                *project = project.replacen(
                    "imports:\n",
                    &format!("imports:\n- from: {{ documents: ['{path}'] }}\n  as: fx\n"),
                    1,
                );
            } else {
                sources.insert(
                    GENERATOR.into(),
                    format!(
                        "import fx from [\"{path}\"];\n{}",
                        generator("builtins.pulse")
                    ),
                );
            }
            let report = check_package_with_overrides(&root(), &sources);
            assert!(report.session.is_none(), "{path}");
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("safe module-relative")),
                "{path}: {:?}",
                report.diagnostics
            );
        }
    }
    for path in ["effects/Upper-name_1.effect.donder", "effects/a b.donder"] {
        assert!(donder_package::validate_module_relative_donder_path(path).is_ok());
    }
}

#[test]
fn linked_target_slots_include_local_imported_and_builtin_children_in_source_order() {
    let mut sources = project_source_texts(&root()).unwrap();
    let emits = ["Local", "fx.ImpactBurst", "builtins.pulse", "Local"]
        .map(|reference| {
            let arguments = match reference {
                "fx.ImpactBurst" => ", gradient: ramp, intensity: shape",
                "builtins.pulse" => ", gradient: ramp, pulse_shape: shape",
                _ => "",
            };
            format!("timeline.emit {reference} {{ start: 0.0, duration: 0.1, target: target{arguments} }};")
        })
        .join("\n");
    sources.insert(GENERATOR.into(), format!("import fx from [\"{CHILD}\"];\neffect Local {{ color sample() {{ return hsv(0.0, 1.0, 1.0); }} }}\neffect MarkImpactBurst {{ param gradient ramp; param curve shape; void generate() {{ {emits} }} }}"));
    // Reach the mutual pair in either order.
    let child = sources[&Utf8PathBuf::from(CHILD)].clone();
    sources.insert(
        CHILD.into(),
        format!("import parent from [\"{GENERATOR}\"];\n{child}"),
    );
    for first in [CHILD, GENERATOR] {
        let mut reordered = sources.clone();
        let project = reordered
            .get_mut(&Utf8PathBuf::from("project.donder"))
            .unwrap();
        *project = project.replacen(
            "imports:\n",
            &format!("imports:\n- from: {{ documents: [{first}] }}\n  as: first\n"),
            1,
        );
        let report = check_package_with_overrides(&root(), &reordered);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let session = report.session.unwrap();
        let definition = session
            .project
            .definitions
            .effects
            .definitions
            .values()
            .find(|definition| definition.source_name == "MarkImpactBurst")
            .unwrap();
        let targets = &definition.generated_effect_targets;
        assert_eq!(targets.len(), 4);
        assert!(
            matches!(&targets[0], EffectRef::Custom(id) if id.0.document() == GENERATOR && id.0.object() == "Local")
        );
        assert!(matches!(&targets[1], EffectRef::Custom(id) if id.0.document() == CHILD));
        assert!(matches!(targets[2], EffectRef::Builtin(_)));
        assert_eq!(targets[0], targets[3]);
    }
}

#[test]
fn edit_visibility_reuses_imports_skips_self_and_allocates_deterministic_aliases() {
    let mut session = donder_project_io::load_package(&root()).unwrap().session;
    let module = session.source.project_module_id();
    let from = DocumentId::new(module, GENERATOR.into());
    let definitions = &session.project.definitions.effects.definitions;
    let own = definitions
        .keys()
        .find(|id| id.0.document_id() == &from)
        .unwrap()
        .0
        .clone();
    let other = definitions
        .keys()
        .find(|id| id.0.document().as_str() != CHILD && id.0.document_id() != &from)
        .unwrap()
        .0
        .clone();
    let count = session.source.documents[&from].imports().len();
    ensure_document_can_reference_source(
        &mut session,
        &from,
        SourceObjectKind::EffectDefinition,
        &own,
    )
    .unwrap();
    assert_eq!(session.source.documents[&from].imports().len(), count);
    ensure_document_can_reference_source(
        &mut session,
        &from,
        SourceObjectKind::EffectDefinition,
        &other,
    )
    .unwrap();
    assert_eq!(
        session.source.documents[&from]
            .imports()
            .last()
            .unwrap()
            .alias(),
        "effects"
    );
    ensure_document_can_reference_source(
        &mut session,
        &from,
        SourceObjectKind::EffectDefinition,
        &other,
    )
    .unwrap();
    assert_eq!(session.source.documents[&from].imports().len(), count + 1);
    let another = session
        .project
        .definitions
        .effects
        .definitions
        .keys()
        .find(|id| id.0 != own && id.0 != other && id.0.document().as_str() != CHILD)
        .unwrap()
        .0
        .clone();
    ensure_document_can_reference_source(
        &mut session,
        &from,
        SourceObjectKind::EffectDefinition,
        &another,
    )
    .unwrap();
    assert_eq!(
        session.source.documents[&from]
            .imports()
            .last()
            .unwrap()
            .alias(),
        "effects_2"
    );
}
