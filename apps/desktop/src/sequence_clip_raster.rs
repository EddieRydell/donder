//! Desktop adapter for frontend sequence-clip raster requests.
//!
//! The worker owns request coalescing, cancellation, cache records, and the
//! pixel-token transport used by the Tauri protocol. Effect evaluation remains
//! in `donder-runtime`; pixel decoding and drawing remain in the frontend.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::mpsc;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::thread;

use donder_elaboration::{PrepareOutputs, prepare};
use donder_language::DonderTime;
use donder_language::compiler::hash_compiled_effect;
use donder_model::DonderProject;
use donder_model::SetupId;
use donder_model::{AutomationBinding, MarkCollectionKey, Sequence, SequenceId};
use donder_model::{
    CurveDefinition, CurveId, CurveSource, EffectDefinition, EffectInst, EffectInstId,
    EffectParamValue, EffectScope, GradientDefinition, GradientId, GradientSource,
};
use donder_project_io::ProjectSession;
use donder_runtime::PreparedSequence;
use donder_runtime_types::{Curve, Gradient};

use donder_sequence_api::{
    EffectRasterSettings, GuiDocumentRequest, SequenceClipRaster, SequenceClipRasterError,
    SequenceClipRasterRequest, SequenceClipRasterResponse, SequenceClipRasterResultBatch,
    SequenceClipRasterUnavailable,
};

const RASTER_CACHE_BYTE_BUDGET: usize = 128 * 1024 * 1024;

mod cache;
mod render;
mod service;
mod signature;
mod worker;

use cache::*;
use render::*;
use signature::*;
use worker::*;

pub(crate) use service::SequenceClipRasterService;
