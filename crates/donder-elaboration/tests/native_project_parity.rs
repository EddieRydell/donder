use donder_elaboration::elaborate_sequence;
use donder_language::dsl::{Identifier, compile_effects};
use donder_language::effect::{EffectDefinition, EffectDefinitionId, EffectRef};
use donder_language::identity::{DocumentId, SourceIdentity};
use donder_language::sequence::AutomationTarget;
use donder_project_io::load_package;

#[test]
fn native_effects_match_reference_dsl_in_real_project_frames() {
    let root = camino::Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let session = load_package(&root).unwrap().session;
    let mut native_project = session.project.clone();
    let automated_sequence_id = native_project
        .root
        .sequences
        .iter()
        .find(|id| id.0.object() == "layer_test")
        .unwrap()
        .clone();
    let sequence = native_project
        .sequences
        .get_mut(&automated_sequence_id)
        .unwrap();
    sequence.automation_clips[0].bindings[0].target = AutomationTarget::EffectParam {
        effect_id: sequence.effects[0].id.clone(),
        param: Identifier::new("pulse_overlap".to_string()).unwrap(),
    };
    let mut reference_project = native_project.clone();
    let document = DocumentId::new(
        session.source.project_module_id(),
        "tests/native-effect-reference.effect.donder".into(),
    );
    for compiled in compile_effects(include_str!(
        "../../donder-language/tests/fixtures/native_effect_reference.effect.donder"
    ))
    .unwrap()
    {
        let id = EffectDefinitionId(SourceIdentity::from_document(
            document.clone(),
            compiled.effect.name().as_str().to_string(),
        ));
        reference_project
            .definitions
            .effects
            .insert(id.clone(), EffectDefinition::custom(id, compiled));
    }
    for sequence in reference_project.sequences.values_mut() {
        for effect in &mut sequence.effects {
            let EffectRef::Builtin(builtin) = effect.definition else {
                continue;
            };
            let name = match builtin {
                donder_language::effect::BuiltinEffect::Pulse => "Pulse",
                donder_language::effect::BuiltinEffect::Chase => "Chase",
                donder_language::effect::BuiltinEffect::Spin => "Spin",
                donder_language::effect::BuiltinEffect::MarkPulse => "MarkPulse",
                donder_language::effect::BuiltinEffect::MarkChase => "MarkChase",
            };
            effect.definition = EffectRef::Custom(EffectDefinitionId(
                SourceIdentity::from_document(document.clone(), name.to_string()),
            ));
        }
    }
    let setup = &session.project.root.setup;
    let sequence = &session.project.root.sequences[0];
    let native = elaborate_sequence(&native_project, setup, sequence).unwrap();
    let reference = elaborate_sequence(&reference_project, setup, sequence).unwrap();
    for frame in [144, 2088, 5904, 9504, 11520, 19080, 7707] {
        assert_eq!(
            native.evaluate_frame(frame).unwrap(),
            reference.evaluate_frame(frame).unwrap(),
            "frame {frame}"
        );
    }
    let native = elaborate_sequence(&native_project, setup, &automated_sequence_id).unwrap();
    let reference = elaborate_sequence(&reference_project, setup, &automated_sequence_id).unwrap();
    assert_eq!(
        native.evaluate_frame(7150).unwrap(),
        reference.evaluate_frame(7150).unwrap()
    );
    let sequence = &session.project.root.sequences[1];
    let native = elaborate_sequence(&native_project, setup, sequence).unwrap();
    let reference = elaborate_sequence(&reference_project, setup, sequence).unwrap();
    assert_eq!(
        native.evaluate_frame(3594).unwrap(),
        reference.evaluate_frame(3594).unwrap()
    );
}
