use crate::common;

use camino::Utf8Path;
use donder_model::{Owned, OwnedMember};
use donder_project_io::{OwnedObjectSlot, PROJECT_ROOT_FILE, check_project, save_project};

const TRANSFORM: &str =
    "Transform { position: (0m, 0m, 0m), rotation: (0.0, 0.0, 0.0), scale: (1.0, 1.0, 1.0) }";

/// Every owned object written in place: a setup with its layout, an inline
/// fixture definition, a patch, two controllers and an input device, a
/// sequence, and a rig.
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
        protocol: E131 {{ sender_name: "Spare", bind_address: "0.0.0.0", priority: 100, mode: Multicast }},
        ports: [Port {{ name: port_1, address: Universe {{ universe: 2 }}, slots: 512 }}],
      }},
    ],
    input_devices: [
      InputDevice pad_5 {{
        description: none,
        driver: Keyboard,
        controls: [Button {{ name: hit, description: none, input: Key {{ code: KeyA }} }}],
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
        protocol: E131 { sender_name: "Test", bind_address: "0.0.0.0", priority: 100, mode: Multicast },
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
      graph: Graph { nodes: [RootNode { position: (0.0, 0.0) }], edges: [] },
      automation: [],
    }"#;

const RIG: &str = r#"  rigs: [
    Rig jam_6 {
      description: none,
      frame_rate: 60,
      marks: [LiveMarks { name: beat, description: none, color: #22c55e }],
      layers: [],
      scene: [],
      switches: [Switch { name: hold, description: none, input: main.setup.input_devices.pad_5.hit }],
      mappings: [
        Mapping {
          name: tap,
          description: none,
          input: main.setup.input_devices.pad_5.hit,
          when: On { switch: hold },
          action: Mark { marks: beat },
        },
      ],
      graph: Graph { nodes: [RootNode { position: (0.0, 0.0) }], edges: [] },
    },
  ],
"#;

fn inline_project() -> String {
    common::root_document("", &inline_fields()).replace("  rigs: [],\n", RIG)
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
    assert!(session.project.reusable::<donder_model::Setup>().is_empty());
    assert!(
        session
            .project
            .reusable::<donder_model::Layout>()
            .is_empty()
    );
    assert!(session.project.reusable::<donder_model::Patch>().is_empty());
    assert!(
        session
            .project
            .reusable::<donder_model::Controller>()
            .is_empty()
    );
    assert!(
        session
            .project
            .reusable::<donder_model::Sequence>()
            .is_empty()
    );
    assert!(
        session
            .project
            .reusable::<donder_model::FixtureDefinition>()
            .is_empty()
    );
    let Owned::Inline(mut setup) = session.project.root().setup.clone() else {
        panic!("expected ownership")
    };
    let address = *setup.controllers[0].id();
    assert_eq!(
        session.address_of(address.into()).unwrap().owned_path(),
        &[
            OwnedObjectSlot::Setup,
            OwnedObjectSlot::Controller(donder_language::object_name("controller_3"))
        ]
    );
    setup.controllers.reverse();
    assert_eq!(setup.controllers[1].id(), &address);
    session.project.put(*setup).unwrap();
    let output = donder_elaboration::prepare(
        &session.project,
        session.project.root().sequences[0].id(),
        donder_elaboration::Retain::All,
    )
    .unwrap();
    assert_eq!(output.outputs().len(), 2);
    save_project(&session).unwrap();
    common::load_project(root);
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
            "a controller appears twice in the setup",
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
        let error = check_project(root, &Default::default(), &mut Default::default())
            .into_session()
            .expect_err("invalid ownership must fail")
            .iter()
            .map(ToString::to_string)
            .collect::<String>();
        assert!(
            error.contains(expected),
            "{error} did not contain {expected}"
        );
    }
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
        assert_eq!(
            session.project.reusable::<donder_model::Controller>().len(),
            1
        );
        let Owned::Inline(mut setup) = session.project.root().setup.clone() else {
            panic!("expected ownership")
        };
        assert!(matches!(
            setup.controllers.remove(1),
            OwnedMember::Reference(_)
        ));
        session.project.put(*setup).unwrap();
        save_project(&session).unwrap();
        let reloaded = common::load_project(root);
        assert_eq!(session.project, reloaded.project);
        assert_eq!(
            reloaded
                .project
                .reusable::<donder_model::Controller>()
                .len(),
            1
        );
    }
}

#[test]
fn every_owned_kind_can_become_reusable_and_independent_without_losing_routes() {
    use donder_model::FixtureInstanceId;
    use donder_model::{make_independent, make_reusable};
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    write_inline_project(root);
    let mut session = common::load_project(root);
    let document = session
        .declaration(session.project.root().id.into())
        .unwrap()
        .0
        .clone();
    let setup = *session.project.root().setup.id();
    let layout = *session
        .project
        .get::<donder_model::Setup>(setup)
        .unwrap()
        .layout
        .id();
    let sites = [
        (
            common::Slot::LayoutFixture {
                layout,
                fixture: FixtureInstanceId(1),
            },
            "strip",
        ),
        (common::Slot::SetupController { setup, index: 0 }, "output"),
        (common::Slot::SetupInputDevice { setup, index: 0 }, "pad"),
        (common::Slot::SetupLayout(setup), "stage"),
        (common::Slot::SetupPatch(setup), "routing"),
        (common::Slot::ProjectSequence(0), "song"),
        (common::Slot::ProjectRig(0), "jam"),
        (common::Slot::ProjectSetup, "venue"),
    ];
    for (site, name) in &sites {
        let site = common::site(&session.project, *site);
        let id = make_reusable(&mut session.project, &site).unwrap();
        session.source.add_object(&document, id, name).unwrap();
        donder_model::validate_project(&session.project).unwrap();
        save_project(&session).unwrap();
        common::load_project(root);
    }
    let reusable_setup = session
        .project
        .get::<donder_model::Setup>(*session.project.root().setup.id())
        .unwrap()
        .clone();
    let site = common::site(&session.project, common::Slot::ProjectSetup);
    make_independent(&mut session.project, &site, &|id| {
        session.source.declared_name(id)
    })
    .unwrap();
    let setup = *session.project.root().setup.id();
    let site = common::site(&session.project, common::Slot::SetupLayout(setup));
    make_independent(&mut session.project, &site, &|id| {
        session.source.declared_name(id)
    })
    .unwrap();
    let site = common::site(
        &session.project,
        common::Slot::SetupController { setup, index: 0 },
    );
    make_independent(&mut session.project, &site, &|id| {
        session.source.declared_name(id)
    })
    .unwrap();
    let site = common::site(&session.project, common::Slot::ProjectSequence(0));
    make_independent(&mut session.project, &site, &|id| {
        session.source.declared_name(id)
    })
    .unwrap();
    let site = common::site(&session.project, common::Slot::ProjectRig(0));
    make_independent(&mut session.project, &site, &|id| {
        session.source.declared_name(id)
    })
    .unwrap();
    let site = common::site(
        &session.project,
        common::Slot::SetupInputDevice { setup, index: 0 },
    );
    make_independent(&mut session.project, &site, &|id| {
        session.source.declared_name(id)
    })
    .unwrap();
    let rig = session
        .project
        .get::<donder_model::Rig>(*session.project.root().rigs[0].id())
        .unwrap();
    let device = session
        .project
        .get::<donder_model::Setup>(setup)
        .unwrap()
        .input_devices[0]
        .id();
    assert!(rig.inputs().all(|input| &input.device == device));
    let owned = session.project.get::<donder_model::Setup>(setup).unwrap();
    assert!(matches!(owned.patch, Owned::Inline(_)));
    let layout = *owned.layout.id();
    let site = common::site(
        &session.project,
        common::Slot::LayoutFixture {
            layout,
            fixture: FixtureInstanceId(1),
        },
    );
    make_independent(&mut session.project, &site, &|id| {
        session.source.declared_name(id)
    })
    .unwrap();
    assert_eq!(
        session
            .project
            .get::<donder_model::Setup>(reusable_setup.id)
            .unwrap(),
        &reusable_setup
    );
    donder_model::validate_project(&session.project).unwrap();
    save_project(&session).unwrap();
    common::load_project(root);
}

#[test]
fn independent_setup_retargets_a_linked_patch_to_its_copied_owned_children() {
    use donder_model::{make_independent, make_reusable};
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    write_inline_project(root);
    let mut session = common::load_project(root);
    let document = session
        .declaration(session.project.root().id.into())
        .unwrap()
        .0
        .clone();
    let setup = *session.project.root().setup.id();
    let site = common::site(&session.project, common::Slot::SetupPatch(setup));
    let patch = make_reusable(&mut session.project, &site).unwrap();
    session
        .source
        .add_object(&document, patch, "routing")
        .unwrap();
    let site = common::site(&session.project, common::Slot::ProjectSetup);
    let source = make_reusable(&mut session.project, &site).unwrap();
    session
        .source
        .add_object(&document, source, "venue")
        .unwrap();
    let shared = session
        .project
        .get::<donder_model::Setup>(*session.project.root().setup.id())
        .unwrap()
        .clone();
    let original_patch = session
        .project
        .get::<donder_model::Patch>(*shared.patch.id())
        .unwrap()
        .clone();
    let site = common::site(&session.project, common::Slot::ProjectSetup);
    make_independent(&mut session.project, &site, &|id| {
        session.source.declared_name(id)
    })
    .unwrap();
    let owned = session
        .project
        .get::<donder_model::Setup>(*session.project.root().setup.id())
        .unwrap();
    let patch = session
        .project
        .get::<donder_model::Patch>(*owned.patch.id())
        .unwrap();
    assert_eq!(&patch.routes[0].target.layout, owned.layout.id());
    assert_eq!(&patch.routes[0].controller, owned.controllers[0].id());
    assert_eq!(
        session
            .project
            .get::<donder_model::Patch>(original_patch.id)
            .unwrap(),
        &original_patch
    );
    assert_eq!(
        session
            .project
            .get::<donder_model::Setup>(shared.id)
            .unwrap(),
        &shared
    );
    donder_model::validate_project(&session.project).unwrap();
    save_project(&session).unwrap();
    common::load_project(root);
}

#[test]
fn independent_layout_copies_active_sequences_and_preserves_the_reusable_originals() {
    use donder_model::make_independent;
    let root = Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut session = common::load_project(&root);
    let setup = *session.project.root().setup.id();
    let original_layout = *session
        .project
        .get::<donder_model::Setup>(setup)
        .unwrap()
        .layout
        .id();
    let originals = session.project.reusable::<donder_model::Sequence>().clone();
    assert!(
        originals
            .values()
            .any(|sequence| !sequence.clips.is_empty())
    );
    let site = common::site(&session.project, common::Slot::SetupLayout(setup));
    make_independent(&mut session.project, &site, &|id| {
        session.source.declared_name(id)
    })
    .unwrap();
    let new_layout = session
        .project
        .get::<donder_model::Setup>(setup)
        .unwrap()
        .layout
        .id();
    assert_ne!(&original_layout, new_layout);
    assert_eq!(
        session.project.reusable::<donder_model::Sequence>(),
        &originals
    );
    for source in &session.project.root().sequences {
        let sequence = session
            .project
            .get::<donder_model::Sequence>(*source.id())
            .unwrap();
        for effect in &sequence.clips {
            assert_eq!(&effect.target.layout, new_layout);
        }
        donder_elaboration::prepare(
            &session.project,
            &sequence.id,
            donder_elaboration::Retain::All,
        )
        .unwrap();
    }
    donder_model::validate_project(&session.project).unwrap();
}

#[test]
fn promoting_an_owned_setup_to_another_file_preserves_nested_identity_and_routing() {
    use donder_model::{make_independent, make_reusable};
    use donder_project_io::ensure_document_can_reference_object;
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8Path::from_path(temporary.path()).unwrap();
    write_inline_project(root);
    let mut session = common::load_project(root);
    let document = session
        .declaration(session.project.root().id.into())
        .unwrap()
        .0
        .clone();
    let site = common::site(&session.project, common::Slot::ProjectSetup);
    let destination = make_reusable(&mut session.project, &site).unwrap();
    session
        .source
        .add_data_document(
            "setups/venue.data.donder".into(),
            vec![(destination, donder_language::object_name("venue"))],
        )
        .unwrap();
    let reusable = *session.project.root().setup.id();
    ensure_document_can_reference_object(&mut session, &document, reusable.into()).unwrap();
    donder_model::validate_project(&session.project).unwrap();
    save_project(&session).unwrap();
    assert_eq!(session.project, common::load_project(root).project);
    let shared = session
        .project
        .get::<donder_model::Setup>(reusable)
        .unwrap()
        .clone();
    let site = common::site(&session.project, common::Slot::ProjectSetup);
    make_independent(&mut session.project, &site, &|id| {
        session.source.declared_name(id)
    })
    .unwrap();
    assert_eq!(
        session
            .project
            .get::<donder_model::Setup>(reusable)
            .unwrap(),
        &shared
    );
    donder_model::validate_project(&session.project).unwrap();
    save_project(&session).unwrap();
    common::load_project(root);
}

#[test]
fn reusable_sequences_cannot_mix_targets_from_different_layouts() {
    use donder_model::make_independent;
    let root = Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut session = common::load_project(&root);
    let setup = *session.project.root().setup.id();
    let site = common::site(&session.project, common::Slot::SetupLayout(setup));
    make_independent(&mut session.project, &site, &|id| {
        session.source.declared_name(id)
    })
    .unwrap();
    let new_layout = *session
        .project
        .get::<donder_model::Setup>(setup)
        .unwrap()
        .layout
        .id();
    let mut sequence = session
        .project
        .reusable::<donder_model::Sequence>()
        .values()
        .find(|sequence| sequence.clips.len() > 1)
        .unwrap()
        .clone();
    std::sync::Arc::make_mut(&mut sequence.clips[0])
        .target
        .layout = new_layout;
    let error = session.project.put(sequence).unwrap_err();
    assert!(
        matches!(
            error,
            donder_model::EditError::Invalid(donder_model::ProjectValidationError {
                problem: donder_model::ValidationProblem::Sequence(
                    donder_model::SequenceValidationError::Clip {
                        problem: donder_model::ClipProblem::MixedLayouts,
                        ..
                    }
                ),
                ..
            })
        ),
        "{error:?}"
    );
}
#[test]
fn making_an_imported_sequence_independent_keeps_its_local_audio() {
    use donder_model::SequenceAudio;
    use donder_model::make_independent;
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
        .get::<donder_model::Sequence>(*session.project.root().sequences[0].id())
        .unwrap()
        .clone();
    let site = common::site(&session.project, common::Slot::ProjectSequence(0));
    make_independent(&mut session.project, &site, &|id| {
        session.source.declared_name(id)
    })
    .unwrap();
    maintain_ownership_sources(&mut session).unwrap();
    let owned = session
        .project
        .get::<donder_model::Sequence>(*session.project.root().sequences[0].id())
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
    assert_eq!(
        session
            .project
            .get::<donder_model::Sequence>(shared.id)
            .unwrap(),
        &shared
    );
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
    use donder_model::LayoutId;
    use donder_model::use_existing;
    use donder_project_io::maintain_ownership_sources;
    let root = Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut session = common::load_project(&root);
    let setup = *session.project.root().setup.id();
    let original = session
        .project
        .get::<donder_model::Setup>(setup)
        .unwrap()
        .clone();
    let original_layout = session
        .project
        .get::<donder_model::Layout>(*original.layout.id())
        .unwrap()
        .clone();
    let original_patch = session
        .project
        .get::<donder_model::Patch>(*original.patch.id())
        .unwrap()
        .clone();
    let original_sequences = session.project.reusable::<donder_model::Sequence>().clone();
    let mut replacement = original_layout.clone();
    replacement.id = LayoutId(session.project.new_id());
    let document = session
        .declaration(original_layout.id.into())
        .unwrap()
        .0
        .clone();
    session
        .source
        .add_object(&document, replacement.id.into(), "alternate")
        .unwrap();
    session
        .project
        .apply_edits([donder_model::ProjectEdit::Put(replacement.clone().into())])
        .unwrap();
    let site = common::site(&session.project, common::Slot::SetupLayout(setup));
    use_existing(&mut session.project, &site, replacement.id.into(), &|id| {
        session.source.declared_name(id)
    })
    .unwrap();
    maintain_ownership_sources(&mut session).unwrap();
    let current = session.project.get::<donder_model::Setup>(setup).unwrap();
    assert_eq!(current.layout.id(), &replacement.id);
    assert!(matches!(current.patch, Owned::Inline(_)));
    let patch = session
        .project
        .get::<donder_model::Patch>(*current.patch.id())
        .unwrap();
    assert!(!patch.routes.is_empty());
    assert!(
        patch
            .routes
            .iter()
            .all(|route| route.target.layout == replacement.id)
    );
    assert_eq!(
        session
            .project
            .get::<donder_model::Layout>(original_layout.id)
            .unwrap(),
        &original_layout
    );
    assert_eq!(
        session
            .project
            .get::<donder_model::Patch>(original_patch.id)
            .unwrap(),
        &original_patch
    );
    assert_eq!(
        session.project.reusable::<donder_model::Sequence>(),
        &original_sequences
    );
    for sequence in &session.project.root().sequences {
        let value = session
            .project
            .get::<donder_model::Sequence>(*sequence.id())
            .unwrap();
        assert!(
            value
                .clips
                .iter()
                .all(|effect| effect.target.layout == replacement.id)
        );
        donder_elaboration::prepare(
            &session.project,
            sequence.id(),
            donder_elaboration::Retain::All,
        )
        .unwrap();
    }
    donder_model::validate_project(&session.project).unwrap();
}
