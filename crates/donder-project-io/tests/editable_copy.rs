mod common;

use camino::{Utf8Path, Utf8PathBuf};
use donder_project_io::{export_editable_project, export_project, load_package};
use std::{collections::BTreeMap, fs};

#[test]
fn imported_show_exports_with_private_effects_nested_imports_and_audio() {
    let starter_root = Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut starter = common::load_project_package(&starter_root);
    let temporary = tempfile::tempdir().unwrap();
    let directory = Utf8PathBuf::from_path_buf(temporary.path().to_path_buf()).unwrap();
    let root = directory.join("show");
    let rig = root.join("rig");
    fs::create_dir_all(&rig).unwrap();
    let sequence_id = starter
        .project
        .root
        .sequences
        .iter()
        .find(|id| {
            let sequence = &starter.project.sequences[*id];
            !sequence.effects.is_empty()
                && matches!(
                    sequence.audio,
                    donder_language::sequence::SequenceAudio::Asset(_)
                )
        })
        .unwrap()
        .clone();
    let custom = starter
        .project
        .definitions
        .effects
        .definitions
        .keys()
        .next()
        .unwrap()
        .clone();
    let color = starter.project.sequences[&sequence_id].layers[0].color;
    let required = starter.project.definitions.effects.definitions[&custom]
        .params
        .iter()
        .filter(|param| param.default.is_none())
        .map(|param| {
            (
                param.name.clone(),
                donder_language::effect::EffectParamValue::initial_for_type(&param.ty, color)
                    .unwrap(),
            )
        })
        .collect();
    let effect = &mut starter
        .project
        .sequences
        .get_mut(&sequence_id)
        .unwrap()
        .effects[0];
    effect.definition = donder_language::effect::EffectRef::Custom(custom.clone());
    effect.param_overrides = required;
    export_project(&starter, &rig).unwrap();
    common::write_project_package(&rig);
    let nested = rig.join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(
        nested.join("extra.effect.donder"),
        "effect Extra { color sample() { return hsv(0.0, 1.0, 1.0); } }",
    )
    .unwrap();
    let mut nested_manifest = donder_package::PackageManifest::read(&rig).unwrap();
    nested_manifest.module_id = uuid::Uuid::new_v4();
    nested_manifest.project = None;
    nested_manifest.assets.clear();
    nested_manifest.exports = BTreeMap::from([(
        "effects".into(),
        donder_package::ExportGroup {
            documents: vec!["extra.effect.donder".into()],
        },
    )]);
    nested_manifest.write(&nested).unwrap();
    let effect_path = rig.join(custom.0.document());
    let effect_source = fs::read_to_string(&effect_path).unwrap();
    fs::write(
        &effect_path,
        format!("import extra from nested.effects;\n{effect_source}"),
    )
    .unwrap();
    let mut rig_manifest = donder_package::PackageManifest::read(&rig).unwrap();
    rig_manifest.project = None;
    rig_manifest.dependencies.insert(
        "nested".into(),
        donder_package::Dependency::Path {
            path: "nested".into(),
        },
    );
    rig_manifest.exports = BTreeMap::from([
        (
            "setup".into(),
            donder_package::ExportGroup {
                documents: vec![starter.project.root.setup.0.document().to_string()],
            },
        ),
        (
            "sequence".into(),
            donder_package::ExportGroup {
                documents: vec![sequence_id.0.document().to_string()],
            },
        ),
    ]);
    rig_manifest.write(&rig).unwrap();
    fs::write(root.join("project.donder"), format!("imports:\n- from: {{ dependency: rig, export: setup }}\n  as: setup\n- from: {{ dependency: rig, export: sequence }}\n  as: sequence\nshow:\n  type: project\n  setup: setup.{}\n  sequences: [sequence.{}]\n", starter.project.root.setup.0.object(), sequence_id.0.object())).unwrap();
    common::write_project_package(&root);
    let mut manifest = donder_package::PackageManifest::read(&root).unwrap();
    manifest.assets.clear();
    manifest.dependencies.insert(
        "rig".into(),
        donder_package::Dependency::Path { path: "rig".into() },
    );
    manifest.write(&root).unwrap();
    donder_package::Lockfile::from_directory(&manifest, &root, "https://registry.donder.dev")
        .unwrap()
        .write(&root)
        .unwrap();
    let original = load_package(&root).unwrap().session;
    let before = original.clone();
    let destination = directory.join("editable");
    let report = export_editable_project(&original, &destination).unwrap();
    assert_eq!(original, before);
    assert!(!report.copied_assets.is_empty());
    let copied = load_package(&destination).unwrap().session;
    assert!(
        copied
            .source
            .source_graph
            .project_module()
            .manifest
            .dependencies
            .is_empty()
    );
    assert!(
        copied
            .source
            .documents
            .keys()
            .all(|id| copied.source.is_project_owned(id))
    );
    assert!(
        copied
            .source
            .documents
            .values()
            .flat_map(|doc| doc.imports())
            .all(|edge| matches!(
                edge.source(),
                donder_project_io::ImportSource::LocalDocuments { .. }
            ))
    );
    let copied_effect = destination
        .join("dependencies")
        .join(rig_manifest.module_id.to_string())
        .join(custom.0.document());
    let text = fs::read_to_string(copied_effect).unwrap();
    assert!(text.contains("import extra from ["));
    assert!(text.ends_with(&effect_source));
    for (old, new) in original
        .source
        .referenced_assets
        .iter()
        .zip(&copied.source.referenced_assets)
    {
        assert_eq!(
            fs::read(&old.absolute_path).unwrap(),
            fs::read(&new.absolute_path).unwrap()
        );
    }
    let output_before = donder_elaboration::PreparedSequenceOutput::prepare(
        &original.project,
        &original.project.root.setup,
        &original.project.root.sequences[0],
    )
    .unwrap();
    let output_after = donder_elaboration::PreparedSequenceOutput::prepare(
        &copied.project,
        &copied.project.root.setup,
        &copied.project.root.sequences[0],
    )
    .unwrap();
    for time in [0.0, 0.5, 2.0] {
        let before = output_before.render_seconds(time).unwrap();
        let after = output_after.render_seconds(time).unwrap();
        assert_eq!(
            before.controller_frames.len(),
            after.controller_frames.len()
        );
        for (old, new) in before
            .controller_frames
            .iter()
            .zip(&after.controller_frames)
        {
            assert_eq!(old.slots, new.slots);
        }
    }
    assert!(export_editable_project(&original, &destination).is_err());
    let mut missing_audio = original.clone();
    missing_audio.source.referenced_assets[0].absolute_path = root.join("missing.wav");
    let failed_destination = directory.join("failed-copy");
    assert!(
        export_editable_project(&missing_audio, &failed_destination)
            .unwrap_err()
            .contains("missing.wav")
    );
    assert!(!failed_destination.exists());
    assert!(fs::read_dir(&directory).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".donder-copy-")
    }));
    assert_eq!(
        fs::read_to_string(effect_path).unwrap(),
        format!("import extra from nested.effects;\n{effect_source}")
    );
}
