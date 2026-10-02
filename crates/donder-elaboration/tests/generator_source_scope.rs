use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, prepare as prepare_sequence};
use donder_language::dsl::Identifier;
use donder_language::effect::{CurveSource, EffectParamValue, EffectRef, GradientSource};
use donder_language::values::DonderDuration;
use donder_language::values::DonderTime;
use donder_project_io::{check_project_with_overrides, project_source_texts};
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
        let report = check_project_with_overrides(&root, &overrides);
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
            .definitions()
            .effects
            .definitions
            .keys()
            .find(|id| id.0.object() == "MarkImpactBurst")
            .unwrap()
            .clone();
        let mut sequence = session
            .project
            .sequences()
            .find(|sequence| !sequence.effects.is_empty())
            .unwrap()
            .clone();
        sequence.effects.truncate(1);
        sequence.automation_clips.clear();
        sequence.effects[0].definition = EffectRef::Custom(generator_id);
        sequence.effects[0].param_overrides.clear();
        let sequence_id = sequence.id.clone();
        session
            .project
            .replace_sequence(&sequence_id, sequence)
            .unwrap();
        let prepared =
            prepare_sequence(&session.project, &sequence_id, PrepareOutputs::All).unwrap();
        assert!(
            !prepared.to_raw_signals().effects.is_empty(),
            "the generator must actually emit"
        );
    }
}

#[test]
fn starter_mark_generator_emits_its_cross_file_child_with_nonempty_inputs() {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut session = donder_project_io::load_project(&root).unwrap();
    let definitions = &session.project.definitions();
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
    let mut sequence = session
        .project
        .sequences()
        .find(|sequence| !sequence.effects.is_empty())
        .unwrap()
        .clone();
    sequence.mark_collections[0].marks = vec![DonderTime(Duration::ZERO)];
    let marks = sequence.mark_collections[0].key.clone();
    sequence.effects.truncate(1);
    sequence.automation_clips.clear();
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
    session
        .project
        .replace_sequence(&sequence_id, sequence)
        .unwrap();
    let prepared = prepare_sequence(&session.project, &sequence_id, PrepareOutputs::All).unwrap();
    assert!(!prepared.to_raw_signals().effects.is_empty());
}

#[test]
fn acyclic_generator_chain_can_exceed_four_levels() {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut sources = project_source_texts(&root).unwrap();
    let generator_path = Utf8PathBuf::from("effects/mark-impact-burst.effect.donder");
    let source = sources.get_mut(&generator_path).unwrap();
    for level in 0..5 {
        source.push_str(&format!(
            "\neffect Chain{level} {{ void generate() {{ timeline.emit Chain{} {{ start: 0.0, duration: 0.1, target: target }}; }} }}",
            level + 1
        ));
    }
    source.push_str("\neffect Chain5 { color sample() { return hsv(0.0, 1.0, 1.0); } }");
    let report = check_project_with_overrides(&root, &sources);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let mut session = report.session.unwrap();
    let generator_id = session
        .project
        .definitions()
        .effects
        .definitions
        .keys()
        .find(|id| id.0.object() == "Chain0")
        .unwrap()
        .clone();
    let mut sequence = session
        .project
        .sequences()
        .find(|sequence| !sequence.effects.is_empty())
        .unwrap()
        .clone();
    sequence.effects.truncate(1);
    sequence.automation_clips.clear();
    sequence.effects[0].definition = EffectRef::Custom(generator_id);
    sequence.effects[0].param_overrides.clear();
    let sequence_id = sequence.id.clone();
    session
        .project
        .replace_sequence(&sequence_id, sequence)
        .unwrap();
    let prepared = prepare_sequence(&session.project, &sequence_id, PrepareOutputs::All).unwrap();
    assert!(!prepared.to_raw_signals().effects.is_empty());
}

#[test]
fn prepared_generator_accepts_more_than_four_thousand_mark_children() {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut sources = project_source_texts(&root).unwrap();
    sources
        .get_mut(&Utf8PathBuf::from("effects/mark-impact-burst.effect.donder"))
        .unwrap()
        .push_str("\neffect Many { fixed param marks beats; void generate() { for (int mark in beats) { timeline.emit ManyChild { start: 0.0, duration: 0.1, target: target }; } } } effect ManyChild { color sample() { return #ffffff; } }");
    let report = check_project_with_overrides(&root, &sources);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let mut session = report.session.unwrap();
    let generator_id = session
        .project
        .definitions()
        .effects
        .definitions
        .keys()
        .find(|id| id.0.object() == "Many")
        .unwrap()
        .clone();
    let mut sequence = session
        .project
        .sequences()
        .find(|sequence| !sequence.effects.is_empty())
        .unwrap()
        .clone();
    sequence.mark_collections[0].marks = vec![DonderTime(Duration::ZERO); 137];
    let marks = sequence.mark_collections[0].key.clone();
    sequence.effects.truncate(1);
    sequence.automation_clips.clear();
    let effect = &mut sequence.effects[0];
    effect.start = DonderTime(Duration::ZERO);
    effect.duration = DonderDuration(Duration::from_secs(1));
    effect.definition = EffectRef::Custom(generator_id);
    effect.param_overrides = [(
        Identifier::new("beats".into()).unwrap(),
        EffectParamValue::Marks(marks),
    )]
    .into_iter()
    .collect();
    let sequence_id = sequence.id.clone();
    session
        .project
        .replace_sequence(&sequence_id, sequence)
        .unwrap();
    let prepared = prepare_sequence(&session.project, &sequence_id, PrepareOutputs::All).unwrap();
    assert!(prepared.to_raw_signals().effects.len() > 4_096);
}
