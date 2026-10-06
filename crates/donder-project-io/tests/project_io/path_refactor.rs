use std::fs;

use crate::common::starter_copy;
use camino::Utf8Path;
use donder_project_io::{apply_path_change, load_project, plan_path_change};

fn move_path(
    session: &donder_project_io::ProjectSession,
    source: &str,
    destination: &str,
) -> donder_project_io::ProjectSession {
    let plan =
        plan_path_change(session, Utf8Path::new(source), Utf8Path::new(destination)).expect("plan");
    assert!(plan.structural);
    apply_path_change(session, &plan).expect("apply")
}

#[test]
fn moves_imported_setup_sequence_effect_and_operator_and_reloads() {
    let (_temporary, root) = starter_copy();
    fs::create_dir(root.join("moved")).expect("directory");
    let mut session = load_project(&root).expect("load");
    for (source, destination) in [
        ("setups/main.setup.donder", "moved/main.setup.donder"),
        (
            "sequences/layer_test.sequence.donder",
            "moved/layer_test.sequence.donder",
        ),
        (
            "effects/impact-burst.effect.donder",
            "moved/impact-burst.effect.donder",
        ),
        (
            "operators/gain.operator.donder",
            "moved/gain.operator.donder",
        ),
    ] {
        session = move_path(&session, source, destination);
    }
    let reloaded = load_project(&root).expect("reload");
    assert_eq!(reloaded.project, session.project);
    assert_eq!(reloaded.source.entrypoint, session.source.entrypoint);
}

#[test]
fn moves_directories_with_documents_and_referenced_audio() {
    let (_temporary, root) = starter_copy();
    fs::create_dir(root.join("library")).expect("directory");
    fs::create_dir_all(root.join("audio")).unwrap();
    fs::write(root.join("audio/test.wav"), b"test audio").unwrap();
    let sequence = root.join("sequences/layer_test.sequence.donder");
    fs::write(
        &sequence,
        fs::read_to_string(&sequence)
            .unwrap()
            .replace("audio: null", "audio: audio/test.wav"),
    )
    .unwrap();
    let session = load_project(&root).expect("load");
    let session = move_path(&session, "effects", "library/effects");
    let session = move_path(&session, "audio", "library/audio");
    assert_eq!(
        session.source.referenced_assets[0].relative_path,
        "library/audio/test.wav"
    );
    assert!(session.source.documents.keys().any(
        |document| document.path() == Utf8Path::new("library/effects/scan-sweep.effect.donder")
    ));
    let reloaded = load_project(&root).expect("reload");
    assert_eq!(reloaded.project, session.project);
}

#[test]
fn rejects_collisions_root_escapes_and_descendant_moves() {
    let (_temporary, root) = starter_copy();
    let session = load_project(&root).expect("load");
    assert!(
        plan_path_change(
            &session,
            Utf8Path::new("project.donder"),
            Utf8Path::new("setups/main.setup.donder")
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
            Utf8Path::new("project.donder"),
            Utf8Path::new("../outside.donder")
        )
        .expect_err("escape")
        .contains("escape")
    );
}
