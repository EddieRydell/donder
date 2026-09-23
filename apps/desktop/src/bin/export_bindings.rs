#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

use std::path::PathBuf;

fn main() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("frontend")
        .join("src")
        .join("generated")
        .join("bindings.ts");

    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        eprintln!("failed to create generated bindings directory: {error}");
        std::process::exit(1);
    }

    if let Err(error) = donder_desktop::bindings::export_typescript(path) {
        eprintln!("failed to export TypeScript bindings: {error}");
        std::process::exit(1);
    }
}
