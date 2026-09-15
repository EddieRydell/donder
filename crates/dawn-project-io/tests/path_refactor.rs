use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_project_io::{apply_path_change, load_package, plan_path_change};
use std::collections::BTreeMap;
use uuid::Uuid;

fn starter_copy() -> (tempfile::TempDir, Utf8PathBuf) {
    let workspace = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .expect("workspace");
    let temporary = tempfile::tempdir().expect("tempdir");
    let root = Utf8Path::from_path(temporary.path())
        .expect("utf8")
        .join("starter");
    copy_tree(&workspace.join("examples/starter"), &root);
    (temporary, root)
}

fn copy_tree(source: &Utf8Path, destination: &Utf8Path) {
    fs::create_dir_all(destination).expect("destination");
    for entry in fs::read_dir(source).expect("source") {
        let entry = entry.expect("entry");
        let name = entry.file_name().into_string().expect("utf8 name");
        let source_path = source.join(&name);
        let destination_path = destination.join(name);
        if entry.file_type().expect("type").is_dir() {
            copy_tree(&source_path, &destination_path);
        } else {
            fs::copy(source_path, destination_path).expect("copy");
        }
    }
}

fn move_path(
    session: &dawn_project_io::ProjectSession,
    source: &str,
    destination: &str,
) -> dawn_project_io::ProjectSession {
    let plan =
        plan_path_change(session, Utf8Path::new(source), Utf8Path::new(destination)).expect("plan");
    assert!(plan.structural);
    apply_path_change(session, &plan).expect("apply")
}

#[test]
fn moves_entrypoint_setup_sequence_effect_and_operator_and_reloads() {
    let (_temporary, root) = starter_copy();
    fs::create_dir(root.join("moved")).expect("directory");
    let mut session = load_package(&root).expect("load").session;
    for (source, destination) in [
        ("project.dawn", "moved/project.dawn"),
        ("setups/main.setup.dawn", "moved/main.setup.dawn"),
        (
            "sequences/layer_test.sequence.dawn",
            "moved/layer_test.sequence.dawn",
        ),
        (
            "effects/impact-burst.effect.dawn",
            "moved/impact-burst.effect.dawn",
        ),
        ("operators/gain.operator.dawn", "moved/gain.operator.dawn"),
    ] {
        session = move_path(&session, source, destination);
    }
    let manifest = dawn_package::PackageManifest::read(&root).expect("manifest");
    assert_eq!(
        manifest.project.expect("project").entrypoint,
        "moved/project.dawn"
    );
    assert!(
        manifest.exports["project"]
            .documents
            .contains(&"moved/project.dawn".to_string())
    );
    let reloaded = load_package(&root).expect("reload").session;
    assert_eq!(reloaded.project, session.project);
    assert_eq!(reloaded.source.entrypoint, session.source.entrypoint);
}

#[test]
fn moves_directories_with_documents_and_declared_assets() {
    let (_temporary, root) = starter_copy();
    fs::create_dir(root.join("library")).expect("directory");
    let session = load_package(&root).expect("load").session;
    let session = move_path(&session, "effects", "library/effects");
    let session = move_path(&session, "audio", "library/audio");
    let manifest = dawn_package::PackageManifest::read(&root).expect("manifest");
    assert!(
        manifest
            .assets
            .contains_key("library/audio/Babokon - All We Are.mp3")
    );
    assert!(
        session
            .source
            .documents
            .keys()
            .any(|document| document.path()
                == Utf8Path::new("library/effects/scan-sweep.effect.dawn"))
    );
    let reloaded = load_package(&root).expect("reload").session;
    assert_eq!(reloaded.project, session.project);
}

#[test]
fn grouped_dsl_path_moves_replace_each_token_and_preserve_all_other_text() {
    let (_temporary, root) = starter_copy();
    let path = root.join("effects/mark-impact-burst.effect.dawn");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(
        root.join("effects/extra.effect.dawn"),
        "effect Extra { color sample() { return hsv(0.0, 1.0, 1.0); } }",
    )
    .unwrap();
    let grouped = original.replace(
        "[\"effects/impact-burst.effect.dawn\"]",
        "[ /* first */ \"effects/impact-burst.effect.dawn\",\n  \"effects/extra.effect.dawn\" /* second */ ]",
    );
    assert_ne!(original, grouped);
    fs::write(&path, &grouped).unwrap();
    let session = load_package(&root).unwrap().session;
    dawn_project_io::save_project(&session).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), grouped);
    let moved = move_path(&session, "effects", "renamed-effects");
    let expected = grouped.replace("\"effects/", "\"renamed-effects/");
    assert_eq!(
        fs::read_to_string(root.join("renamed-effects/mark-impact-burst.effect.dawn")).unwrap(),
        expected
    );
    let reloaded = load_package(&root).unwrap().session;
    assert_eq!(moved.project, reloaded.project);
    for (id, document) in &moved.source.documents {
        assert_eq!(document.imports(), reloaded.source.documents[id].imports());
    }
}

#[test]
fn rejects_collisions_root_escapes_and_descendant_moves() {
    let (_temporary, root) = starter_copy();
    let session = load_package(&root).expect("load").session;
    assert!(
        plan_path_change(
            &session,
            Utf8Path::new("project.dawn"),
            Utf8Path::new("setups/main.setup.dawn")
        )
        .expect_err("collision")
        .contains("Destination already exists")
    );
    assert!(
        plan_path_change(
            &session,
            Utf8Path::new("effects"),
            Utf8Path::new("effects/nested")
        )
        .expect_err("descendant")
        .contains("descendants")
    );
    assert!(
        plan_path_change(
            &session,
            Utf8Path::new("project.dawn"),
            Utf8Path::new("../outside.dawn")
        )
        .expect_err("escape")
        .contains("escape")
    );
}

#[test]
#[allow(clippy::permissions_set_readonly_false)]
fn failed_commit_restores_source_and_active_files() {
    let (_temporary, root) = starter_copy();
    fs::create_dir(root.join("moved")).expect("directory");
    let session = load_package(&root).expect("load").session;
    let plan = plan_path_change(
        &session,
        Utf8Path::new("effects/impact-burst.effect.dawn"),
        Utf8Path::new("moved/impact-burst.effect.dawn"),
    )
    .expect("plan");
    let protected = root.join("project.dawn");
    let mut permissions = fs::metadata(&protected).expect("metadata").permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&protected, permissions.clone()).expect("readonly");
    let result = apply_path_change(&session, &plan);
    permissions.set_readonly(false);
    fs::set_permissions(&protected, permissions).expect("writable");
    assert!(result.is_err());
    assert!(root.join("effects/impact-burst.effect.dawn").is_file());
    assert!(!root.join("moved/impact-burst.effect.dawn").exists());
    assert!(load_package(&root).is_ok());
}

#[test]
fn moves_local_and_nested_path_dependency_roots_without_changing_module_ids() {
    let (_temporary, root) = starter_copy();
    let local_root = root.join("modules/local");
    let nested_root = root.join("modules/local/nested");
    fs::create_dir_all(&local_root).expect("local");
    fs::create_dir_all(&nested_root).expect("nested");
    fs::copy(
        root.join("effects/impact-burst.effect.dawn"),
        local_root.join("local.effect.dawn"),
    )
    .expect("local effect");
    fs::copy(
        root.join("effects/scan-sweep.effect.dawn"),
        nested_root.join("nested.effect.dawn"),
    )
    .expect("nested effect");
    let local_id = Uuid::new_v4();
    let nested_id = Uuid::new_v4();
    package_manifest(
        local_id,
        "local.effect.dawn",
        BTreeMap::from([(
            "nested".to_string(),
            dawn_package::Dependency::Path {
                path: "nested".to_string(),
            },
        )]),
    )
    .write(&local_root)
    .expect("local manifest");
    package_manifest(nested_id, "nested.effect.dawn", BTreeMap::new())
        .write(&nested_root)
        .expect("nested manifest");
    let mut root_manifest = dawn_package::PackageManifest::read(&root).expect("root manifest");
    root_manifest.dependencies.insert(
        "local".to_string(),
        dawn_package::Dependency::Path {
            path: "modules/local".to_string(),
        },
    );
    root_manifest.write(&root).expect("write root manifest");
    let original_lock = dawn_package::Lockfile::read(&root).expect("original lock");
    dawn_package::Lockfile::from_directory(&root_manifest, &root, original_lock.registry.clone())
        .expect("path lock")
        .write(&root)
        .expect("write lock");
    fs::create_dir(root.join("libraries")).expect("destination parent");

    let nested_path = Utf8PathBuf::from("modules/local/nested/nested.effect.dawn");
    let original = fs::read_to_string(root.join(&nested_path)).unwrap();
    let edited = original.replace("param float repeats = 1.0;", "param float repeats = 3.0;");
    assert_ne!(edited, original);
    let report = dawn_project_io::check_package_with_overrides(
        &root,
        &BTreeMap::from([(nested_path.clone(), edited.clone())]),
    );
    let unsaved = report.session.expect("path dependency override compiles");
    let document = unsaved
        .source
        .document_for_workspace_path(&nested_path)
        .unwrap();
    assert_eq!(document.module_id(), nested_id);
    assert_eq!(
        dawn_project_io::source_document_text(&unsaved, &document)
            .unwrap()
            .unwrap(),
        edited
    );
    assert_eq!(
        fs::read_to_string(root.join(&nested_path)).unwrap(),
        original
    );

    let session = load_package(&root).expect("load").session;
    let plan = plan_path_change(
        &session,
        Utf8Path::new("modules"),
        Utf8Path::new("libraries/modules"),
    )
    .expect("plan");
    assert_eq!(plan.impact.modules.len(), 2);
    let candidate = apply_path_change(&session, &plan).expect("apply");
    let lock = dawn_package::Lockfile::read(&root).expect("lock");
    assert_eq!(
        lock.path_dependencies["libraries/modules/local"].module_id,
        local_id
    );
    assert_eq!(
        lock.path_dependencies["libraries/modules/local/nested"].module_id,
        nested_id
    );
    assert_eq!(lock.registry, original_lock.registry);
    let reloaded = load_package(&root).expect("reload").session;
    assert_eq!(reloaded.project, candidate.project);
}

fn package_manifest(
    module_id: Uuid,
    document: &str,
    dependencies: BTreeMap<String, dawn_package::Dependency>,
) -> dawn_package::PackageManifest {
    dawn_package::PackageManifest {
        manifest_version: dawn_package::MANIFEST_VERSION,
        module_id,
        language_version: dawn_package::LANGUAGE_VERSION.to_string(),
        project: None,
        publication: None,
        exports: BTreeMap::from([(
            "effects".to_string(),
            dawn_package::ExportGroup {
                documents: vec![document.to_string()],
            },
        )]),
        dependencies,
        assets: BTreeMap::new(),
    }
}

#[test]
fn generator_dependency_exports_keep_module_identity_and_follow_path_changes() {
    let (_temporary, root) = starter_copy();
    let local_root = root.join("modules/local");
    fs::create_dir_all(&local_root).unwrap();
    let child = fs::read_to_string(root.join("effects/impact-burst.effect.dawn")).unwrap();
    fs::write(local_root.join("child.effect.dawn"), &child).unwrap();
    let module_id = Uuid::new_v4();
    let mut dependency_manifest = package_manifest(module_id, "child.effect.dawn", BTreeMap::new());
    let effects = dependency_manifest.exports.remove("effects").unwrap();
    dependency_manifest
        .exports
        .insert("child-effects".into(), effects);
    dependency_manifest.write(&local_root).unwrap();
    let mut manifest = dawn_package::PackageManifest::read(&root).unwrap();
    manifest.dependencies.insert(
        "local-effects".into(),
        dawn_package::Dependency::Path {
            path: "modules/local".into(),
        },
    );
    manifest.write(&root).unwrap();
    let registry = dawn_package::Lockfile::read(&root).unwrap().registry;
    dawn_package::Lockfile::from_directory(&manifest, &root, registry)
        .unwrap()
        .write(&root)
        .unwrap();
    let generator_path = root.join("effects/mark-impact-burst.effect.dawn");
    let source = fs::read_to_string(&generator_path).unwrap().replace(
        "[\"effects/impact-burst.effect.dawn\"]",
        "local-effects.child-effects",
    );
    fs::write(&generator_path, &source).unwrap();
    let session = load_package(&root).unwrap().session;
    let generator = session
        .project
        .definitions
        .effects
        .definitions
        .iter()
        .find(|(id, _)| id.0.object() == "MarkImpactBurst")
        .unwrap()
        .1;
    let dawn_language::effect::EffectRef::Custom(target) =
        generator.generated_effect_targets.first().unwrap()
    else {
        panic!("expected custom generated target")
    };
    assert_eq!(target.0.document_id().module_id(), module_id);
    assert_eq!(target.0.document(), Utf8Path::new("child.effect.dawn"));
    let mut equivalent = dawn_project_io::project_source_texts(&root).unwrap();
    let project = equivalent
        .get_mut(&Utf8PathBuf::from("project.dawn"))
        .unwrap();
    *project = project.replacen(
        "imports:\n",
        "imports:\n- from: { dependency: local-effects, export: child-effects }\n  as: bursts\n",
        1,
    );
    let report = dawn_project_io::check_package_with_overrides(&root, &equivalent);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let equivalent = report.session.unwrap();
    let root_module = equivalent.source.project_module_id();
    let yaml = &equivalent.source.documents
        [&dawn_language::identity::DocumentId::new(root_module, "project.dawn".into())]
        .imports()[0];
    let dsl = &equivalent.source.documents[&dawn_language::identity::DocumentId::new(
        root_module,
        "effects/mark-impact-burst.effect.dawn".into(),
    )]
        .imports()[0];
    assert_eq!(yaml, dsl);
    for (old, replacement) in [
        ("child-effects", "unknown-export"),
        ("local-effects", "Invalid_Name"),
    ] {
        let mut overrides = dawn_project_io::project_source_texts(&root).unwrap();
        let invalid = source.replacen(old, replacement, 1);
        overrides.insert(
            "effects/mark-impact-burst.effect.dawn".into(),
            invalid.clone(),
        );
        let report = dawn_project_io::check_package_with_overrides(&root, &overrides);
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.path == "effects/mark-impact-burst.effect.dawn")
            .unwrap();
        let range = diagnostic.range.as_ref().unwrap();
        assert_eq!(range.start.line, 0);
        assert_eq!(
            range.start.character as usize,
            invalid.find(replacement).unwrap()
        );
        assert_eq!(
            range.end.character - range.start.character,
            replacement.len() as u32
        );
    }
    let mut no_declared_export = session.clone();
    let from = dawn_language::identity::DocumentId::new(
        session.source.project_module_id(),
        "project.dawn".into(),
    );
    assert!(
        dawn_project_io::ensure_document_can_reference_source(
            &mut no_declared_export,
            &from,
            dawn_project_io::SourceObjectKind::EffectDefinition,
            &target.0
        )
        .is_err()
    );
    dawn_project_io::save_project(&session).unwrap();
    assert_eq!(
        child,
        fs::read_to_string(local_root.join("child.effect.dawn")).unwrap()
    );
    assert_eq!(source, fs::read_to_string(&generator_path).unwrap());
    assert_eq!(
        session.project,
        load_package(&root).unwrap().session.project
    );
    let moved = move_path(
        &session,
        "modules/local/child.effect.dawn",
        "modules/local/renamed.effect.dawn",
    );
    assert_eq!(source, fs::read_to_string(generator_path).unwrap());
    assert_eq!(moved.project, load_package(&root).unwrap().session.project);
    let generator = moved
        .project
        .definitions
        .effects
        .definitions
        .iter()
        .find(|(id, _)| id.0.object() == "MarkImpactBurst")
        .unwrap()
        .1;
    let dawn_language::effect::EffectRef::Custom(target) =
        generator.generated_effect_targets.first().unwrap()
    else {
        panic!("expected custom generated target")
    };
    assert_eq!(target.0.document(), Utf8Path::new("renamed.effect.dawn"));
}
