use camino::Utf8PathBuf;
use donder_elaboration::PreparedSequenceOutput;
use donder_language::dsl::Identifier;
use donder_language::effect::{CurveSource, EffectParamValue, EffectRef, GradientSource};
use donder_language::values::DonderTime;
use donder_project_io::{check_package_with_overrides, project_source_texts};
use std::time::Duration;

#[test]
fn explicit_generator_imports_and_local_children_prepare_but_callers_scope_is_not_inherited() {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let original = project_source_texts(&root).unwrap();
    let generator_path = Utf8PathBuf::from("effects/mark-impact-burst.effect.donder");
    let child_path = Utf8PathBuf::from("effects/impact-burst.effect.donder");
    let generator = "effect MarkImpactBurst { void generate() { timeline.emit ImpactBurst { start: 0.0, duration: 0.1, target: target }; } }";
    let child = "effect ImpactBurst { color sample() { return hsv(progress(), 1.0, 1.0); } }";
    enum Scope {
        Local,
        Imported,
        MutualImports,
        MissingImport,
    }
    for scope in [
        Scope::Local,
        Scope::Imported,
        Scope::MutualImports,
        Scope::MissingImport,
    ] {
        let mut overrides = original.clone();
        let (generator_source, child_source) = match scope {
            Scope::Local => (
                format!("{generator}\n{child}"),
                child.replace("ImpactBurst", "UnrelatedChild"),
            ),
            Scope::Imported | Scope::MutualImports => (
                format!(
                    "import bursts from [\"effects/impact-burst.effect.donder\"];\n{}",
                    generator.replace("emit ImpactBurst", "emit bursts.ImpactBurst")
                ),
                if matches!(scope, Scope::MutualImports) {
                    format!(
                        "import generators from [\"effects/mark-impact-burst.effect.donder\"];\n{child}"
                    )
                } else {
                    child.into()
                },
            ),
            Scope::MissingImport => (generator.into(), child.into()),
        };
        overrides.insert(generator_path.clone(), generator_source);
        overrides.insert(child_path.clone(), child_source);
        let report = check_package_with_overrides(&root, &overrides);
        if matches!(scope, Scope::MissingImport) {
            assert!(report.session.is_none());
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.path == generator_path
                        && diagnostic
                            .message
                            .contains("generated child reference `ImpactBurst`")),
                "{:?}",
                report.diagnostics
            );
            continue;
        }
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let mut session = report.session.unwrap();
        let generator_id = session
            .project
            .definitions
            .effects
            .definitions
            .keys()
            .find(|id| id.0.object() == "MarkImpactBurst")
            .unwrap()
            .clone();
        let sequence = session
            .project
            .sequences
            .values_mut()
            .find(|sequence| !sequence.effects.is_empty())
            .unwrap();
        sequence.effects.truncate(1);
        sequence.effects[0].definition = EffectRef::Custom(generator_id);
        sequence.effects[0].param_overrides.clear();
        let sequence_id = sequence.id.clone();
        let prepared = PreparedSequenceOutput::prepare(
            &session.project,
            &session.project.root.setup,
            &sequence_id,
        )
        .unwrap();
        assert!(
            !prepared.sequence.signals.effects.is_empty(),
            "the generator must actually emit"
        );
    }
}

#[test]
fn starter_mark_generator_emits_its_cross_file_child_with_nonempty_inputs() {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut session = donder_project_io::load_package(&root).unwrap().session;
    let definitions = &session.project.definitions;
    let generator_id = definitions
        .effects
        .definitions
        .keys()
        .find(|id| id.0.object() == "MarkImpactBurst")
        .unwrap()
        .clone();
    let gradient = definitions
        .gradients
        .definitions
        .keys()
        .next()
        .unwrap()
        .clone();
    let curve = definitions
        .curves
        .definitions
        .keys()
        .next()
        .unwrap()
        .clone();
    let sequence = session
        .project
        .sequences
        .values_mut()
        .find(|sequence| !sequence.effects.is_empty())
        .unwrap();
    sequence.mark_collections[0].marks = vec![DonderTime(Duration::ZERO)];
    let marks = sequence.mark_collections[0].key.clone();
    sequence.effects.truncate(1);
    let effect = &mut sequence.effects[0];
    effect.start = DonderTime(Duration::ZERO);
    effect.definition = EffectRef::Custom(generator_id);
    effect.param_overrides = [
        ("beats", EffectParamValue::Marks(marks)),
        (
            "gradients",
            EffectParamValue::Array(vec![EffectParamValue::Gradient(GradientSource::Reference(
                gradient,
            ))]),
        ),
        (
            "intensity",
            EffectParamValue::Curve(CurveSource::Reference(curve)),
        ),
    ]
    .into_iter()
    .map(|(name, value)| (Identifier::new(name.into()).unwrap(), value))
    .collect();
    let sequence_id = sequence.id.clone();
    let prepared = PreparedSequenceOutput::prepare(
        &session.project,
        &session.project.root.setup,
        &sequence_id,
    )
    .unwrap();
    assert!(!prepared.sequence.signals.effects.is_empty());
}
