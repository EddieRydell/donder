use crate::common;

use camino::Utf8Path;
use donder_model::{Owned, OwnedMember};
use donder_project_io::{OwnedObjectSlot, PROJECT_ROOT_FILE, check_project, save_project};

const TRANSFORM: &str =
    "Transform { position: (0m, 0m, 0m), rotation: (0.0, 0.0, 0.0), scale: (1.0, 1.0, 1.0) }";

/// Every owned object written in place: a setup with its layout, an inline
/// fixture definition, a patch, two controllers and an input device, a
/// sequence, and a console.
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
          port: "1",
          start: Next,
          encoding: Rgb {{ order: (0, 1, 2) }},
          gamma: 1.0,
          brightness: 1.0,
          dither: false,
        }},
      ],
    }},
    controllers: [
      {CONTROLLER},
      Controller spare_8 {{
        description: none,
        protocol: E131 {{ sender_name: "Spare", bind_address: "0.0.0.0", priority: 100, mode: Multicast, start_universe: 2, universes: 1, channels: 512, fseq_channel: none, ports: [] }},
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
        protocol: E131 {
          sender_name: "Test",
          bind_address: "0.0.0.0",
          priority: 100,
          mode: Multicast,
          start_universe: 1,
          universes: 1,
          channels: 512,
          fseq_channel: none,
          ports: [Port { name: "1", description: none }],
        },
      }"#;

const SEQUENCE: &str = r#"Sequence song_4 {
      description: none,
      duration: 1s,
      frame_rate: 30,
      audio: none,
      marks: [],
      layers: [Layer { name: default, description: none, color: #50a0ff, enabled: true }],
      clips: [],
      graph: Graph { nodes: [RootNode { position: (0.0, 0.0) }], edges: [] },
      automation: [],
    }"#;

const CONSOLE: &str = r#"  consoles: [
    Console jam_6 {
      description: none,
      frame_rate: 60,
      marks: [LiveMarks { name: beat, description: none, color: #22c55e }],
      layers: [Layer { name: default, description: none, color: #50a0ff, enabled: true }],
      clips: [],
      always: [],
      events: [
        On {
          event: Press { control: main.setup.input_devices.pad_5.hit },
          actions: [Mark { into: beat }],
        },
      ],
      graph: Graph { nodes: [RootNode { position: (0.0, 0.0) }], edges: [] },
    },
  ],
"#;

fn inline_project() -> String {
    common::root_document("", &inline_fields()).replace("  consoles: [],\n", CONSOLE)
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
        (
            fields.replace("port: \"1\"", "port: \"9\""),
            "no port \"9\"",
        ),
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
