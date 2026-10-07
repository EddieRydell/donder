//! Audio feature boundary: transport state in `engine`, Kira adaptation in `backend`.

mod backend;
mod engine;

pub(crate) use engine::{AudioEngine, DeviceBoundary};
