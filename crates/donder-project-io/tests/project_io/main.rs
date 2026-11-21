//! donder-project-io integration tests, in one binary so the shared helpers
//! are compiled once.
mod common;
mod diagnostics;
mod exact_round_trip;
mod fixture_composition;
mod imports;
mod live_input;
mod local_project;
mod ownership;
mod path_refactor;
mod roundtrip;
mod schema_strictness;
