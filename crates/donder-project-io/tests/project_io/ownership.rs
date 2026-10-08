use crate::common;

use camino::Utf8Path;
use donder_language::identity::OwnedObjectSlot;
use donder_language::ownership::ValueSource;
use donder_project_io::{PROJECT_ROOT_FILE, load_project, save_project};

const TRANSFORM: &str =
    "Transform { position: (0m, 0m, 0m), rotation: (0.0, 0.0, 0.0), scale: (1.0, 1.0, 1.0) }";

/// Every owned object written in place: a setup with its layout, an inline
/// fixture definition, a patch, two controllers, and a sequence.
fn inline_fields() -> String {
    format!(
        r#"  setup: Setup {{
    description: none,
    layout: Layout {{
      description: none,
      root: [test_strip],
      items: [
        Fixture {{
          name: test_strip,
          description: none,
          definition: FixtureDefinition {{
            description: none,
            shapes: [Shape {{ name: pixel, diameter: 0.01m, reverse: false, transform: {TRANSFORM}, geometry: Pixel }}],
          }},
          transform: {TRANSFORM},
        }},
      ],
    }},
    patch: Patch {{
      description: none,
      routes: [
        Route {{
          target: main.setup.layout.test_strip,
          pixels: none,
          controller: main.setup.controllers.controller_3,
          port: port_1,
          start_slot: 0,
          encoding: Rgb {{ order: (0, 1, 2) }},
          gamma: 1.0,
          brightness: 1.0,
        }},
      ],
    }},
    controllers: [
      {CONTROLLER},
      Controller spare_8 {{
        description: none,
        protocol: E131 {{ source_name: "Spare", bind_address: "0.0.0.0", priority: 100, mode: Multicast }},
        ports: [Port {{ name: port_1, address: Universe {{ universe: 2 }}, slots: 512 }}],
      }},
    ],
  }},
  sequences: [
    {SEQUENCE},
  ],
"#
    )
}

const CONTROLLER: &str = r#"Controller controller_3 {
        description: none,
        protocol: E131 { source_name: "Test", bind_address: "0.0.0.0", priority: 100, mode: Multicast },
        ports: [Port { name: port_1, address: Universe { universe: 1 }, slots: 512 }],
      }"#;

const SEQUENCE: &str = r#"Sequence song_4 {
      description: none,
      duration: 1s,
      frame_rate: 30,
      audio: none,
      marks: [],
      layers: [],
      clips: [],
      graph: Graph { nodes: [OutputNode { position: (0.0, 0.0) }], edges: [] },
      automation: [],
    }"#;

fn inline_project() -> String {
    common::root_document("", &inline_fields())
}

fn write_inline_project(root: &Utf8Path) {
    std::fs::write(root.join(PROJECT_ROOT_FILE), inline_project()).unwrap();
}

#[test]
fn nested_objects_roundtrip_without_named_sibling_definitions() {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    write_inline_project(root);
    let mut session = common::load_project(root);
    assert!(session.project.reusable_setups().is_empty());
    assert!(session.project.reusable_layouts().is_empty());
    assert!(session.project.reusable_patches().is_empty());
    assert!(session.project.reusable_controllers().is_empty());
    assert!(session.project.reusable_sequences().is_empty());
    assert!(
        session
            .project
            .definitions()
            .fixtures
            .definitions
            .is_empty()
    );
    let ValueSource::Inline(mut setup) = session.project.root().setup.clone() else {
        panic!("expected ownership")
    };
    let address = setup.controllers[0].id().clone();
    assert_eq!(
        address.0.owned_path(),
        &[
            OwnedObjectSlot::Setup,
            OwnedObjectSlot::Controller(donder_language::names::object_name("controller_3"))
        ]
    );
    setup.controllers.reverse();
    assert_eq!(setup.controllers[1].id(), &address);
    session
        .project
        .replace_setup(&setup.id.clone(), *setup)
        .unwrap();
    let output = donder_elaboration::prepare(
        &session.project,
        session.project.root().sequences[0].id(),
        donder_elaboration::PrepareOutputs::All,
    )
    .unwrap();
    assert_eq!(output.outputs().len(), 2);
    save_project(&session).unwrap();
    let reloaded = common::load_project(root);
    assert_eq!(session.project, reloaded.project);
    let text = std::fs::read_to_string(root.join(PROJECT_ROOT_FILE)).unwrap();
    let (document, diagnostics) = donder_language::data::parse(&text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(document.declarations.len(), 1);
    assert_eq!(document.declarations[0].name.value.as_str(), "main");
}

#[test]
fn ownership_rejects_duplicate_names_malformed_types_and_dangling_addresses() {
    let fields = inline_fields();
    for (text, expected) in [
        (
            fields.replace("Controller spare_8", "Controller controller_3"),
            "appears more than once",
        ),
        (
            fields.replace(
                "controller: main.setup.controllers.controller_3",
                "controller: main.setup.controllers.controller_99",
            ),
            "main.setup.controllers.controller_99",
        ),
        (
            fields.replace(
                "target: main.setup.layout.test_strip",
                "target: main.layout.test_strip",
            ),
            "main.layout",
        ),
        (
            fields.replace("layout: Layout {", "layout: Patch {"),
            "Layout",
        ),
        (
            fields.replace("frame_rate: 30,", "frame_rate: 30,\n      typo: true,"),
            "typo",
        ),
        (fields.replace("port: port_1", "port: port_9"), "port_9"),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(temporary.path()).unwrap();
        std::fs::write(
            root.join(PROJECT_ROOT_FILE),
            common::root_document("", &text),
        )
        .unwrap();
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
    write_inline_project(root);
    let mut session = common::load_project(root);
    let before = session.project.root().id.0.document_id().clone();
    let after = DocumentId::new(before.module_id(), "renamed/project.data.donder".into());
    donder_language::source_remap::remap_document_paths(
        &mut session.project,
        &[(before, after.clone())].into(),
    )
    .unwrap();
    donder_language::validation::validate_project(&session.project).unwrap();
    let setup = session
        .project
        .setup(session.project.root().setup.id())
        .unwrap();
    assert_eq!(setup.id.0.document_id(), &after);
    let route = &session.project.patch(setup.patch.id()).unwrap().routes[0];
    assert_eq!(&route.target.layout, setup.layout.id());
    assert_eq!(&route.controller, setup.controllers[0].id());
}

#[test]
fn same_file_and_other_file_links_preserve_reusable_objects_after_detaching() {
    let spare_start = inline_fields().find("      Controller spare_8").unwrap();
    let spare_end = inline_fields()[spare_start..].find("      },\n").unwrap() + spare_start + 9;
    for separate_file in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(temporary.path()).unwrap();
        let fields = inline_fields();
        let spare = fields[spare_start..spare_end]
            .trim()
            .trim_end_matches(',')
            .replace("Controller spare_8", "Controller spare");
        let (imports, reference) = if separate_file {
            std::fs::write(root.join("controllers.data.donder"), format!("{spare}\n")).unwrap();
            (
                "import controllers from <controllers.data.donder>;\n",
                "controllers.spare",
            )
        } else {
            ("", "spare")
        };
        let fields = format!(
            "{}      {reference},\n{}",
            &fields[..spare_start],
            &fields[spare_end..]
        );
        let mut text = common::root_document(imports, &fields);
        if !separate_file {
            text.push_str(&format!("\n{spare}\n"));
        }
        std::fs::write(root.join(PROJECT_ROOT_FILE), text).unwrap();
        let mut session = common::load_project(root);
        assert_eq!(session.project.reusable_controllers().len(), 1);
        let ValueSource::Inline(mut setup) = session.project.root().setup.clone() else {
            panic!("expected ownership")
        };
        assert!(matches!(
            setup.controllers.remove(1),
            ValueSource::Reference(_)
        ));
        session
            .project
            .replace_setup(&setup.id.clone(), *setup)
            .unwrap();
        save_project(&session).unwrap();
        let reloaded = common::load_project(root);
        assert_eq!(session.project, reloaded.project);
        assert_eq!(reloaded.project.reusable_controllers().len(), 1);
    }
}

#[test]
fn every_owned_kind_can_become_reusable_and_independent_without_losing_routes() {
    use donder_language::layout::FixtureInstanceId;
    use donder_language::ownership::edit::{OwnershipSite, make_independent, make_reusable};
    use donder_project_io::SourceObjectKind;
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    write_inline_project(root);
    let mut session = common::load_project(root);
    let document = session.project.root().id.0.document_id().clone();
    let setup = session.project.root().setup.id().clone();
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
        .setup(session.project.root().setup.id())
        .unwrap()
        .clone();
    make_independent(&mut session.project, &OwnershipSite::ProjectSetup).unwrap();
    let setup = session.project.root().setup.id().clone();
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
    write_inline_project(root);
    let mut session = common::load_project(root);
    let document = session.project.root().id.0.document_id().clone();
    let setup = session.project.root().setup.id().clone();
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
        .setup(session.project.root().setup.id())
        .unwrap()
        .clone();
    let original_patch = session.project.patch(shared.patch.id()).unwrap().clone();
    make_independent(&mut session.project, &OwnershipSite::ProjectSetup).unwrap();
    let owned = session
        .project
        .setup(session.project.root().setup.id())
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
    let setup = session.project.root().setup.id().clone();
    let original_layout = session.project.setup(&setup).unwrap().layout.id().clone();
    let originals = session.project.reusable_sequences().clone();
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
    assert_eq!(session.project.reusable_sequences(), &originals);
    for source in &session.project.root().sequences {
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
    write_inline_project(root);
    let mut session = common::load_project(root);
    let document = session.project.root().id.0.document_id().clone();
    let destination = session
        .source
        .add_data_document(
            "setups/venue.data.donder".into(),
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
    let reusable = session.project.root().setup.id().clone();
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
    let setup = session.project.root().setup.id().clone();
    make_independent(
        &mut session.project,
        &OwnershipSite::SetupLayout(setup.clone()),
    )
    .unwrap();
    let new_layout = session.project.setup(&setup).unwrap().layout.id().clone();
    let mut sequence = session
        .project
        .reusable_sequences()
        .values()
        .find(|sequence| sequence.effects.len() > 1)
        .unwrap()
        .clone();
    sequence.effects[0].target.layout = new_layout;
    let error = session
        .project
        .replace_sequence(&sequence.id.clone(), sequence)
        .unwrap_err();
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
    std::fs::write(
        library.join("songs.data.donder"),
        format!(
            "{}\n",
            SEQUENCE
                .replace("Sequence song_4", "Sequence song")
                .replace("audio: none", "audio: <library/song.wav>")
        ),
    )
    .unwrap();
    std::fs::write(library.join("song.wav"), b"test audio asset").unwrap();
    let fields = inline_fields().replace(SEQUENCE, "songs.song");
    std::fs::write(
        root.join(PROJECT_ROOT_FILE),
        common::root_document("import songs from <library/songs.data.donder>;\n", &fields),
    )
    .unwrap();
    let mut session = common::load_project(root);
    let shared = session
        .project
        .sequence(session.project.root().sequences[0].id())
        .unwrap()
        .clone();
    make_independent(&mut session.project, &OwnershipSite::ProjectSequence(0)).unwrap();
    maintain_ownership_sources(&mut session).unwrap();
    let owned = session
        .project
        .sequence(session.project.root().sequences[0].id())
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
        std::fs::read_to_string(root.join(PROJECT_ROOT_FILE))
            .unwrap()
            .contains("<library/song.wav>")
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
    let setup = session.project.root().setup.id().clone();
    let original = session.project.setup(&setup).unwrap().clone();
    let original_layout = session
        .project
        .layout(original.layout.id())
        .unwrap()
        .clone();
    let original_patch = session.project.patch(original.patch.id()).unwrap().clone();
    let original_sequences = session.project.reusable_sequences().clone();
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
        .apply_edits([donder_language::model::ProjectEdit::InsertLayout(
            replacement.clone(),
        )])
        .unwrap();
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
    assert_eq!(session.project.reusable_sequences(), &original_sequences);
    for sequence in &session.project.root().sequences {
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
