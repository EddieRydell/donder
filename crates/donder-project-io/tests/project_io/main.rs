//! donder-project-io integration tests, in one binary so the shared helpers
//! are compiled once.
mod common;
mod diagnostics;
mod editable_copy;
mod fixture_composition;
mod imports;
mod local_project;
mod ownership;
mod path_refactor;
mod roundtrip;
mod schema_strictness;
mod semantic_preservation;
