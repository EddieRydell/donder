mod common;

use camino::Utf8Path;
use donder_language::identity::OwnedObjectSlot;
use donder_language::ownership::ValueSource;
use donder_project_io::{load_project, save_project};

const INLINE_PROJECT: &str = r#"
show:
  type: project
  setup:
    type: setup
    layout:
      type: layout
      fixtures:
      - id: 1
        name: Test strip
        type: fixture
        definition:
          type: fixture
          elements:
          - id: 1
            name: Pixel
            reverse: false
            shape: {type: pixel}
            diameter: 0.01
    patch:
      type: patch
      routes:
      - id: 1
        target:
          layout: {owner: show, path: [setup, layout]}
          fixture: 1
        controller: {owner: show, path: [setup, {type: controller, id: 3}]}
        port: 1
        start_slot: 0
        encoding: {type: rgb, order: [0, 1, 2]}
        gamma: 1
        brightness: 1
    controllers:
    - type: controller
      id: 3
      protocol: {type: e131, source_name: Test, bind_address: 0.0.0.0, priority: 100, mode: multicast}
      ports: [{id: 1, slot_count: 512, universe: 1}]
    - type: controller
      id: 8
      protocol: {type: e131, source_name: Spare, bind_address: 0.0.0.0, priority: 100, mode: multicast}
      ports: [{id: 1, slot_count: 512, universe: 2}]
  sequences:
  - type: sequence
    id: 4
    duration: 1s
    frame_rate: 30
    audio: null
    layers: []
    effects: []
    composition_graph:
      nodes: [{id: 1, position: {x: 0, y: 0}, type: output}]
      edges: []
"#;

#[test]
fn nested_objects_roundtrip_without_named_sibling_definitions() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    std::fs::write(root.join("project.donder"), INLINE_PROJECT).unwrap();
    common::write_workspace_metadata(root);
    let mut session = common::load_project(root);
    assert!(session.project.setups.is_empty());
    assert!(session.project.layouts.is_empty());
    assert!(session.project.patches.is_empty());
    assert!(session.project.controllers.is_empty());
    assert!(session.project.sequences.is_empty());
    assert!(session.project.definitions.fixtures.definitions.is_empty());
    let ValueSource::Inline(setup) = &mut session.project.root.setup else {
        panic!("expected ownership")
    };
    let address = setup.controllers[0].id().clone();
    assert_eq!(
        address.0.owned_path(),
        &[OwnedObjectSlot::Setup, OwnedObjectSlot::Controller(3)]
    );
    setup.controllers.reverse();
    assert_eq!(setup.controllers[1].id(), &address);
    let output = donder_elaboration::prepare(
        &session.project,
        session.project.root.sequences[0].id(),
        donder_elaboration::PrepareOutputs::All,
    )
    .unwrap();
    assert_eq!(output.outputs().len(), 2);
    save_project(&session).unwrap();
    let reloaded = common::load_project(root);
    assert_eq!(session.project, reloaded.project);
    let text = std::fs::read_to_string(root.join("project.donder")).unwrap();
    let document: yaml_serde::Value = yaml_serde::from_str(&text).unwrap();
    assert_eq!(document.as_mapping().unwrap().len(), 2);
    assert!(document.as_mapping().unwrap().contains_key("workspace"));
    assert!(document.as_mapping().unwrap().contains_key("show"));
}

#[test]
fn ownership_rejects_duplicate_ids_malformed_types_and_dangling_addresses() {
    for (text, expected) in [
        (
            INLINE_PROJECT.replace("id: 8", "id: 3"),
            "Controller appears more than once",
        ),
        (
            INLINE_PROJECT.replace("type: controller, id: 3", "type: controller, id: 99"),
            "MissingController",
        ),
        (
            INLINE_PROJECT.replace("path: [setup, layout]", "path: [layout]"),
            "Invalid ownership path",
        ),
        (
            INLINE_PROJECT.replace("type: layout", "type: patch"),
            "Expected type `layout`",
        ),
        (
            INLINE_PROJECT.replace("    id: 4", "    id: 4\n    typo: true"),
            "unknown field `typo`",
        ),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(temporary.path()).unwrap();
        std::fs::write(root.join("project.donder"), text).unwrap();
        common::write_workspace_metadata(root);
        let error = load_project(root)
            .expect_err("invalid ownership must fail")
            .to_string();
        assert!(
            error.contains(expected),
            "{error} did not contain {expected}"
        );
    }
}

#[test]
fn document_moves_keep_owned_targets_attached_to_their_owner() {
    use donder_language::identity::DocumentId;
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    std::fs::write(root.join("project.donder"), INLINE_PROJECT).unwrap();
    common::write_workspace_metadata(root);
    let mut session = common::load_project(root);
    let before = session.project.root.id.0.document_id().clone();
    let after = DocumentId::new(before.module_id(), "renamed/project.donder".into());
    donder_language::source_remap::remap_document_paths(
        &mut session.project,
        &[(before, after.clone())].into(),
    );
    donder_language::validation::validate_project(&session.project).unwrap();
    let setup = session
        .project
        .setup(session.project.root.setup.id())
        .unwrap();
    assert_eq!(setup.id.0.document_id(), &after);
    let route = &session.project.patch(setup.patch.id()).unwrap().routes[0];
    assert_eq!(&route.target.layout, setup.layout.id());
    assert_eq!(&route.controller, setup.controllers[0].id());
}

#[test]
fn same_file_and_other_file_links_preserve_reusable_objects_after_detaching() {
    for separate_file in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(temporary.path()).unwrap();
        let mut document: yaml_serde::Value = yaml_serde::from_str(INLINE_PROJECT).unwrap();
        let controller = document["show"]["setup"]["controllers"]
            .as_sequence_mut()
            .unwrap()
            .remove(1);
        let mut controller = controller.as_mapping().unwrap().clone();
        controller.remove(yaml_serde::Value::String("id".into()));
        let mut reusable = yaml_serde::Mapping::new();
        reusable.insert("spare".into(), yaml_serde::Value::Mapping(controller));
        let reference = if separate_file {
            std::fs::write(
                root.join("controllers.donder"),
                yaml_serde::to_string(&reusable).unwrap(),
            )
            .unwrap();
            document["imports"] = yaml_serde::from_str(
                "[{from: {documents: [controllers.donder]}, as: controllers}]",
            )
            .unwrap();
            "controllers.spare"
        } else {
            document.as_mapping_mut().unwrap().extend(reusable);
            "spare"
        };
        document["show"]["setup"]["controllers"]
            .as_sequence_mut()
            .unwrap()
            .push(reference.into());
        std::fs::write(
            root.join("project.donder"),
            yaml_serde::to_string(&document).unwrap(),
        )
        .unwrap();
        common::write_workspace_metadata(root);
        let mut session = common::load_project(root);
        assert_eq!(session.project.controllers.len(), 1);
        let ValueSource::Inline(setup) = &mut session.project.root.setup else {
            panic!("expected ownership")
        };
        assert!(matches!(
            setup.controllers.remove(1),
            ValueSource::Reference(_)
        ));
        save_project(&session).unwrap();
        let reloaded = common::load_project(root);
        assert_eq!(session.project, reloaded.project);
        assert_eq!(reloaded.project.controllers.len(), 1);
    }
}

#[test]
fn saving_a_dangling_owned_target_fails_before_writing() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    std::fs::write(root.join("project.donder"), INLINE_PROJECT).unwrap();
    common::write_workspace_metadata(root);
    let before = std::fs::read_to_string(root.join("project.donder")).unwrap();
    let mut session = common::load_project(root);
    let setup = session.project.root.setup.inline_mut().unwrap();
    let controller = setup.controllers[1].id().clone();
    setup.patch.inline_mut().unwrap().routes[0].controller = controller;
    setup.controllers.remove(1);
    assert!(save_project(&session).is_err());
    assert_eq!(
        std::fs::read_to_string(root.join("project.donder")).unwrap(),
        before
    );
}
#[test]
fn every_owned_kind_can_become_reusable_and_independent_without_losing_routes() {
    use donder_language::layout::FixtureInstanceId;
    use donder_language::ownership::edit::{OwnershipSite, make_independent, make_reusable};
    use donder_project_io::SourceObjectKind;
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    std::fs::write(root.join("project.donder"), INLINE_PROJECT).unwrap();
    common::write_workspace_metadata(root);
    let mut session = common::load_project(root);
    let document = session.project.root.id.0.document_id().clone();
    let setup = session.project.root.setup.id().clone();
    let layout = session.project.setup(&setup).unwrap().layout.id().clone();
    let sites = [
        (
            OwnershipSite::LayoutFixture {
                layout,
                fixture: FixtureInstanceId(1),
            },
            SourceObjectKind::FixtureDefinition,
            "strip",
        ),
        (
            OwnershipSite::SetupController {
                setup: setup.clone(),
                index: 0,
            },
            SourceObjectKind::Controller,
            "output",
        ),
        (
            OwnershipSite::SetupLayout(setup.clone()),
            SourceObjectKind::Layout,
            "stage",
        ),
        (
            OwnershipSite::SetupPatch(setup),
            SourceObjectKind::Patch,
            "routing",
        ),
        (
            OwnershipSite::ProjectSequence(0),
            SourceObjectKind::Sequence,
            "song",
        ),
        (
            OwnershipSite::ProjectSetup,
            SourceObjectKind::Setup,
            "venue",
        ),
    ];
    for (site, kind, name) in &sites {
        let destination = session
            .source
            .add_object(&document, kind.clone(), name)
            .unwrap();
        make_reusable(&mut session.project, site, destination).unwrap();
        donder_language::validation::validate_project(&session.project).unwrap();
        save_project(&session).unwrap();
        assert_eq!(session.project, common::load_project(root).project);
    }
    let reusable_setup = session
        .project
        .setup(session.project.root.setup.id())
        .unwrap()
        .clone();
    make_independent(&mut session.project, &OwnershipSite::ProjectSetup).unwrap();
    let setup = session.project.root.setup.id().clone();
    make_independent(
        &mut session.project,
        &OwnershipSite::SetupLayout(setup.clone()),
    )
    .unwrap();
    make_independent(
        &mut session.project,
        &OwnershipSite::SetupController {
            setup: setup.clone(),
            index: 0,
        },
    )
    .unwrap();
    make_independent(&mut session.project, &OwnershipSite::ProjectSequence(0)).unwrap();
    let owned = session.project.setup(&setup).unwrap();
    assert!(matches!(owned.patch, ValueSource::Inline(_)));
    let layout = owned.layout.id().clone();
    make_independent(
        &mut session.project,
        &OwnershipSite::LayoutFixture {
            layout,
            fixture: FixtureInstanceId(1),
        },
    )
    .unwrap();
    assert_eq!(
        session.project.setup(&reusable_setup.id).unwrap(),
        &reusable_setup
    );
    donder_language::validation::validate_project(&session.project).unwrap();
    save_project(&session).unwrap();
    assert_eq!(session.project, common::load_project(root).project);
}

#[test]
fn independent_setup_retargets_a_linked_patch_to_its_copied_owned_children() {
    use donder_language::ownership::edit::{OwnershipSite, make_independent, make_reusable};
    use donder_project_io::SourceObjectKind;
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    std::fs::write(root.join("project.donder"), INLINE_PROJECT).unwrap();
    common::write_workspace_metadata(root);
    let mut session = common::load_project(root);
    let document = session.project.root.id.0.document_id().clone();
    let setup = session.project.root.setup.id().clone();
    let patch = session
        .source
        .add_object(&document, SourceObjectKind::Patch, "routing")
        .unwrap();
    make_reusable(
        &mut session.project,
        &OwnershipSite::SetupPatch(setup),
        patch,
    )
    .unwrap();
    let source = session
        .source
        .add_object(&document, SourceObjectKind::Setup, "venue")
        .unwrap();
    make_reusable(&mut session.project, &OwnershipSite::ProjectSetup, source).unwrap();
    let shared = session
        .project
        .setup(session.project.root.setup.id())
        .unwrap()
        .clone();
    let original_patch = session.project.patch(shared.patch.id()).unwrap().clone();
    make_independent(&mut session.project, &OwnershipSite::ProjectSetup).unwrap();
    let owned = session
        .project
        .setup(session.project.root.setup.id())
        .unwrap();
    let patch = session.project.patch(owned.patch.id()).unwrap();
    assert_eq!(&patch.routes[0].target.layout, owned.layout.id());
    assert_eq!(&patch.routes[0].controller, owned.controllers[0].id());
    assert_eq!(
        session.project.patch(&original_patch.id).unwrap(),
        &original_patch
    );
    assert_eq!(session.project.setup(&shared.id).unwrap(), &shared);
    donder_language::validation::validate_project(&session.project).unwrap();
    save_project(&session).unwrap();
    assert_eq!(session.project, common::load_project(root).project);
}

#[test]
fn independent_layout_copies_active_sequences_and_preserves_the_reusable_originals() {
    use donder_language::ownership::edit::{OwnershipSite, make_independent};
    let root = Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut session = common::load_project(&root);
    let setup = session.project.root.setup.id().clone();
    let original_layout = session.project.setup(&setup).unwrap().layout.id().clone();
    let originals = session.project.sequences.clone();
    assert!(
        originals
            .values()
            .any(|sequence| !sequence.effects.is_empty())
    );
    make_independent(
        &mut session.project,
        &OwnershipSite::SetupLayout(setup.clone()),
    )
    .unwrap();
    let new_layout = session.project.setup(&setup).unwrap().layout.id();
    assert_ne!(&original_layout, new_layout);
    assert_eq!(session.project.sequences, originals);
    for source in &session.project.root.sequences {
        let sequence = session.project.sequence(source.id()).unwrap();
        for effect in &sequence.effects {
            assert_eq!(&effect.target.layout, new_layout);
        }
        donder_elaboration::prepare(
            &session.project,
            &sequence.id,
            donder_elaboration::PrepareOutputs::All,
        )
        .unwrap();
    }
    donder_language::validation::validate_project(&session.project).unwrap();
}

#[test]
fn promoting_an_owned_setup_to_another_file_preserves_nested_identity_and_routing() {
    use donder_language::ownership::edit::{OwnershipSite, make_independent, make_reusable};
    use donder_project_io::{SourceObjectKind, ensure_document_can_reference_object};
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    std::fs::write(root.join("project.donder"), INLINE_PROJECT).unwrap();
    common::write_workspace_metadata(root);
    let mut session = common::load_project(root);
    let document = session.project.root.id.0.document_id().clone();
    let destination = session
        .source
        .add_yaml_document(
            "setups/venue.donder".into(),
            vec![(SourceObjectKind::Setup, "venue".into())],
        )
        .unwrap()
        .remove(0);
    make_reusable(
        &mut session.project,
        &OwnershipSite::ProjectSetup,
        destination,
    )
    .unwrap();
    let reusable = session.project.root.setup.id().clone();
    ensure_document_can_reference_object(&mut session, &document, &reusable.0).unwrap();
    donder_language::validation::validate_project(&session.project).unwrap();
    save_project(&session).unwrap();
    assert_eq!(session.project, common::load_project(root).project);
    let shared = session.project.setup(&reusable).unwrap().clone();
    make_independent(&mut session.project, &OwnershipSite::ProjectSetup).unwrap();
    assert_eq!(session.project.setup(&reusable).unwrap(), &shared);
    donder_language::validation::validate_project(&session.project).unwrap();
    save_project(&session).unwrap();
    assert_eq!(session.project, common::load_project(root).project);
}

#[test]
fn reusable_sequences_cannot_mix_targets_from_different_layouts() {
    use donder_language::ownership::edit::{OwnershipSite, make_independent};
    let root = Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut session = common::load_project(&root);
    let setup = session.project.root.setup.id().clone();
    make_independent(
        &mut session.project,
        &OwnershipSite::SetupLayout(setup.clone()),
    )
    .unwrap();
    let new_layout = session.project.setup(&setup).unwrap().layout.id().clone();
    let sequence = session
        .project
        .sequences
        .values_mut()
        .find(|sequence| sequence.effects.len() > 1)
        .unwrap();
    sequence.effects[0].target.layout = new_layout;
    let error = donder_language::validation::validate_project(&session.project).unwrap_err();
    assert!(error.to_string().contains("same layout"), "{error}");
}
#[test]
fn making_an_imported_sequence_independent_keeps_its_local_audio() {
    use donder_language::ownership::edit::{OwnershipSite, make_independent};
    use donder_language::sequence::SequenceAudio;
    use donder_project_io::maintain_ownership_sources;
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    let library = root.join("library");
    std::fs::create_dir(&library).unwrap();
    let mut source: yaml_serde::Value = yaml_serde::from_str(INLINE_PROJECT).unwrap();
    let mut sequence = source["show"]["sequences"][0].clone();
    sequence
        .as_mapping_mut()
        .unwrap()
        .remove(yaml_serde::Value::String("id".into()));
    sequence["audio"] = "library/song.wav".into();
    let library_doc = yaml_serde::Mapping::from_iter([("song".into(), sequence)]);
    std::fs::write(
        library.join("project.donder"),
        yaml_serde::to_string(&library_doc).unwrap(),
    )
    .unwrap();
    std::fs::write(library.join("song.wav"), b"test audio asset").unwrap();
    source["show"]["sequences"] = yaml_serde::Value::Sequence(vec!["songs.song".into()]);
    let project_doc = format!(
        "imports:\n- from: {{ documents: [library/project.donder] }}\n  as: songs\n{}",
        yaml_serde::to_string(&source).unwrap()
    );
    std::fs::write(root.join("project.donder"), project_doc).unwrap();
    common::write_workspace_metadata(root);
    let mut session = common::load_project(root);
    let shared = session
        .project
        .sequence(session.project.root.sequences[0].id())
        .unwrap()
        .clone();
    make_independent(&mut session.project, &OwnershipSite::ProjectSequence(0)).unwrap();
    maintain_ownership_sources(&mut session).unwrap();
    let owned = session
        .project
        .sequence(session.project.root.sequences[0].id())
        .unwrap();
    assert_eq!(owned.audio, shared.audio);
    let SequenceAudio::Asset(id) = &owned.audio else {
        panic!("asset missing")
    };
    let asset = session
        .source
        .referenced_assets
        .iter()
        .find(|asset| &asset.id == id)
        .unwrap();
    assert_eq!(asset.referenced_by.len(), 2);
    assert_eq!(session.project.sequence(&shared.id).unwrap(), &shared);
    save_project(&session).unwrap();
    let reloaded = common::load_project(root);
    assert_eq!(session.project, reloaded.project);
    assert_eq!(
        session.source.referenced_assets,
        reloaded.source.referenced_assets
    );
    assert!(
        std::fs::read_to_string(root.join("project.donder"))
            .unwrap()
            .contains("library/song.wav")
    );
    assert_eq!(
        std::fs::read(library.join("song.wav")).unwrap(),
        b"test audio asset"
    );
}
#[test]
fn selecting_another_layout_retargets_only_the_current_setup_and_active_sequences() {
    use donder_language::layout::LayoutId;
    use donder_language::ownership::edit::{OwnershipSite, use_existing};
    use donder_project_io::{SourceObjectKind, maintain_ownership_sources};
    let root = Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut session = common::load_project(&root);
    let setup = session.project.root.setup.id().clone();
    let original = session.project.setup(&setup).unwrap().clone();
    let original_layout = session
        .project
        .layout(original.layout.id())
        .unwrap()
        .clone();
    let original_patch = session.project.patch(original.patch.id()).unwrap().clone();
    let original_sequences = session.project.sequences.clone();
    let source = session
        .source
        .add_object(
            original_layout.id.0.document_id(),
            SourceObjectKind::Layout,
            "alternate",
        )
        .unwrap();
    let mut replacement = original_layout.clone();
    replacement.id = LayoutId(source.clone().into());
    session
        .project
        .layouts
        .insert(replacement.id.clone(), replacement.clone());
    use_existing(
        &mut session.project,
        &OwnershipSite::SetupLayout(setup.clone()),
        source,
    )
    .unwrap();
    maintain_ownership_sources(&mut session).unwrap();
    let current = session.project.setup(&setup).unwrap();
    assert_eq!(current.layout.id(), &replacement.id);
    assert!(matches!(current.patch, ValueSource::Inline(_)));
    let patch = session.project.patch(current.patch.id()).unwrap();
    assert!(!patch.routes.is_empty());
    assert!(
        patch
            .routes
            .iter()
            .all(|route| route.target.layout == replacement.id)
    );
    assert_eq!(
        session.project.layout(&original_layout.id).unwrap(),
        &original_layout
    );
    assert_eq!(
        session.project.patch(&original_patch.id).unwrap(),
        &original_patch
    );
    assert_eq!(session.project.sequences, original_sequences);
    for sequence in &session.project.root.sequences {
        let value = session.project.sequence(sequence.id()).unwrap();
        assert!(
            value
                .effects
                .iter()
                .all(|effect| effect.target.layout == replacement.id)
        );
        donder_elaboration::prepare(
            &session.project,
            sequence.id(),
            donder_elaboration::PrepareOutputs::All,
        )
        .unwrap();
    }
    donder_language::validation::validate_project(&session.project).unwrap();
}
