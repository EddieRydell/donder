#![deny(unsafe_code)]
#![deny(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]

use donder_sequence_api::{SequenceGuiEdit, SequenceSelectionEdit};
use specta::Types;
use specta_serde::Format;
use specta_typescript::{Typescript, semantic};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bindings.ts");
    let types = Types::default()
        .register::<SequenceGuiEdit>()
        .register::<SequenceSelectionEdit>();
    let semantic_types = semantic::Configuration::default()
        .enable_lossless_floats()
        .apply_types(&types);
    Typescript::default().export_to(&path, &semantic_types, Format)?;
    Ok(())
}
