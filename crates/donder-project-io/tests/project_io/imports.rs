use crate::common;

use camino::Utf8PathBuf;
use donder_language::identity::DocumentId;
use donder_language::imports::ImportAlias;
use donder_project_io::{
    PROJECT_ROOT_FILE, SourceObjectKind, check_project, check_project_with_overrides,
    ensure_document_can_reference_source, project_source_texts,
};

fn root() -> Utf8PathBuf {
    common::starter_root()
}

/// The starter's sources, with `EFFECT` no longer imported by the root.
fn sources() -> donder_project_io::SourceOverrides {
    let mut sources = project_source_texts(&root()).unwrap();
    let project = sources
        .get_mut(&Utf8PathBuf::from(PROJECT_ROOT_FILE))
        .unwrap();
    *project = project
        .replace("\r\n", "\n")
        .replace(&format!("<{EFFECT}>, "), "");
    sources
}

const EFFECT: &str = "effects/impact-burst.donder";
const EXTRA: &str = "effects/import-test.donder";

/// A minimal project whose only imports are `imports`, with `EFFECT`
/// defining `ImpactBurst` and `EXTRA` holding `extra`.
fn tiny_project(imports: &str, extra: &str) -> (tempfile::TempDir, Utf8PathBuf) {
    let temporary = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temporary.path().to_path_buf()).unwrap();
    std::fs::create_dir(root.join("effects")).unwrap();
    std::fs::write(
        root.join(EFFECT),
        "effect ImpactBurst { sample { hsv(0.0, 1.0, 1.0) } }",
    )
    .unwrap();
    std::fs::write(root.join(EXTRA), extra).unwrap();
    std::fs::write(
        root.join(PROJECT_ROOT_FILE),
        common::root_document(
            imports,
            "  setup: Setup {\n    description: none,\n    layout: Layout { description: none, root: [], items: [] },\n    patch: Patch { description: none, routes: [] },\n    controllers: [],\n  },\n  sequences: [],\n",
        ),
    )
    .unwrap();
    (temporary, root)
}

#[test]
fn grouped_declarations_preserve_ordered_targets() {
    let aliases = [
        "Fx",
        "_fx2",
        "an_alias_longer_than_thirty_two_bytes_is_valid",
    ];
    for alias in aliases {
        assert!(ImportAlias::new(alias).is_ok(), "{alias}");
    }
    let alias = aliases[2];
    let (_temporary, root) = tiny_project(
        &format!("import {alias} from <{EFFECT}>, <{EXTRA}>;\n"),
        "effect Extra { sample { hsv(0.0, 1.0, 1.0) } }",
    );
    let report = check_project(&root);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let session = report.session.unwrap();
    let module = session.source.project_module_id();
    let edge =
        &session.source.documents[&DocumentId::new(module, PROJECT_ROOT_FILE.into())].imports()[0];
    assert_eq!(edge.alias(), alias);
    assert_eq!(
        edge.targets()
            .iter()
            .map(|id| id.path().as_str())
            .collect::<Vec<_>>(),
        [EFFECT, EXTRA]
    );
}

#[test]
fn invalid_aliases_are_rejected() {
    for alias in ["builtins", "effect", "if", "1fx", "with-hyphen", "é", ""] {
        assert!(ImportAlias::new(alias).is_err(), "{alias}");
    }
    // The loader validates aliases through `ImportAlias::new`.
    let (_temporary, root) = tiny_project(
        &format!("import builtins from <{EFFECT}>;\n"),
        "effect Extra { sample { hsv(0.0, 1.0, 1.0) } }",
    );
    assert!(check_project(&root).session.is_none());
}

#[test]
fn grouped_collisions_report_both_source_occurrences() {
    for (imports, message) in [
        (
            format!("import fx from <{EFFECT}>, <{EFFECT}>;\n"),
            "imported more than once",
        ),
        (
            format!("import fx from <{EFFECT}>;\nimport other from <{EFFECT}>;\n"),
            "imported more than once",
        ),
        (
            format!("import fx from <{EFFECT}>;\nimport fx from <{EXTRA}>;\n"),
            "duplicate import alias",
        ),
        (
            format!("import fx from <{EFFECT}>, <{EXTRA}>;\n"),
            "duplicate exported object",
        ),
    ] {
        let (_temporary, root) = tiny_project(
            &imports,
            "effect ImpactBurst { sample { hsv(0.0, 1.0, 1.0) } }",
        );
        let report = check_project(&root);
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.message.contains(message))
            .unwrap_or_else(|| panic!("{:?}", report.diagnostics));
        assert_eq!(diagnostic.path, PROJECT_ROOT_FILE);
        assert!(diagnostic.range.is_some());
        assert_eq!(diagnostic.related.len(), 1);
        assert!(diagnostic.related[0].range.is_some());
        assert_ne!(diagnostic.range, diagnostic.related[0].range);
    }
}

#[test]
fn missing_group_member_points_at_its_own_path_token() {
    let mut sources = sources();
    let project = sources
        .get_mut(&Utf8PathBuf::from(PROJECT_ROOT_FILE))
        .unwrap();
    let missing = "<effects/missing.donder>";
    *project = format!("import fx from <{EFFECT}>, {missing};\n{project}");
    let start = project.find(missing).unwrap() as u32;
    let report = check_project_with_overrides(&root(), &sources);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("target does not exist"))
        .unwrap_or_else(|| panic!("{:?}", report.diagnostics));
    let range = diagnostic.range.as_ref().unwrap();
    assert_eq!((range.start.line, range.start.character), (0, start));
    assert_eq!(
        range.end.character - range.start.character,
        missing.len() as u32
    );
}

#[test]
fn imported_names_are_not_transitive_and_wrong_kinds_do_not_resolve() {
    for reference in ["setups.outputs_layout", "effects.Pulse", "missing.main"] {
        let mut sources = sources();
        let project = sources
            .get_mut(&Utf8PathBuf::from(PROJECT_ROOT_FILE))
            .unwrap();
        assert!(project.contains("setup: setups.main"));
        *project = project.replace("setup: setups.main", &format!("setup: {reference}"));
        let report = check_project_with_overrides(&root(), &sources);
        assert!(report.session.is_none(), "{reference}");
        assert!(
            report.diagnostics.iter().any(
                |diagnostic| diagnostic.path == PROJECT_ROOT_FILE && diagnostic.range.is_some()
            ),
            "{:?}",
            report.diagnostics
        );
    }
}

#[test]
fn local_imports_use_safe_document_paths() {
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
            donder_project_io::validate_document_path(path).is_err(),
            "{path}"
        );
    }
    // The loader validates import paths through `validate_document_path`.
    let (_temporary, root) = tiny_project(
        "import fx from <../escape.donder>;\n",
        "effect Extra { sample { hsv(0.0, 1.0, 1.0) } }",
    );
    let report = check_project(&root);
    assert!(report.session.is_none());
    assert!(
        report.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("safe project-relative")
            && diagnostic.range.is_some()),
        "{:?}",
        report.diagnostics
    );
    for path in ["effects/Upper-name_1.donder", "effects/a b.data.donder"] {
        assert!(donder_project_io::validate_document_path(path).is_ok());
    }
}

#[test]
fn mutual_data_imports_work_in_either_traversal_order() {
    for first in ["curves/a.data.donder", "curves/b.data.donder"] {
        let mut sources = sources();
        for (own, other) in [("a", "b"), ("b", "a")] {
            sources.insert(
                format!("curves/{own}.data.donder").into(),
                format!(
                    "import other from <curves/{other}.data.donder>;\n\nCurve {own} {{ description: none, points: [(0.0, 1.0)] }}\n"
                ),
            );
        }
        let project = sources
            .get_mut(&Utf8PathBuf::from(PROJECT_ROOT_FILE))
            .unwrap();
        *project = format!("import curves from <{first}>;\n{project}");
        let report = check_project_with_overrides(&root(), &sources);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    }
}

#[test]
fn edit_visibility_reuses_imports_skips_self_and_allocates_deterministic_aliases() {
    let mut session = donder_project_io::load_project(&root()).unwrap();
    let module = session.source.project_module_id();
    let from = DocumentId::new(module, "sequences/empty.data.donder".into());
    let own = session
        .project
        .sequences()
        .find(|sequence| sequence.id.0.document_id() == &from)
        .unwrap()
        .id
        .0
        .root_source()
        .clone();
    let count = session.source.documents[&from].imports().len();
    ensure_document_can_reference_source(&mut session, &from, SourceObjectKind::Sequence, &own)
        .unwrap();
    assert_eq!(session.source.documents[&from].imports().len(), count);
    let definitions = &session.project.definitions().effects.definitions;
    let existing = definitions
        .keys()
        .find(|id| id.0.document().as_str() == "effects/standard.donder")
        .unwrap()
        .0
        .clone();
    let another = definitions
        .keys()
        .find(|id| id.0.document().as_str() == "effects/mark-impact-burst.donder")
        .unwrap()
        .0
        .clone();
    ensure_document_can_reference_source(
        &mut session,
        &from,
        SourceObjectKind::EffectDefinition,
        &existing,
    )
    .unwrap();
    assert_eq!(session.source.documents[&from].imports().len(), count);
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
    ensure_document_can_reference_source(
        &mut session,
        &from,
        SourceObjectKind::EffectDefinition,
        &another,
    )
    .unwrap();
    assert_eq!(session.source.documents[&from].imports().len(), count + 1);
}
