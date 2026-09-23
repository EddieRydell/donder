//! Desktop adapter for frontend sequence-clip raster requests.
//!
//! The worker owns request coalescing, cancellation, cache records, and the
//! pixel-token transport used by the Tauri protocol. Effect evaluation remains
//! in `donder-elaboration`; pixel decoding and drawing remain in the frontend.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::mpsc;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::thread;

use donder_elaboration::{
    EffectRasterPrepareBatch, PreparedEffectRasterRenderer, RenderedTargetPixelAddress,
    resolve_effect_target_pixel_addresses,
};
use donder_language::dsl::{EffectKind, hash_compiled_effect};
use donder_language::effect::{
    CurveDefinition, CurveId, CurveSource, EffectDefinition, EffectInst, EffectInstId,
    EffectParamValue, EffectScope, GradientDefinition, GradientId, GradientSource,
};
use donder_language::model::DonderProject;
use donder_language::sequence::{
    AutomationBinding, AutomationMapping, MarkCollectionKey, Sequence, SequenceId,
};
use donder_language::setup::SetupId;
use donder_language::values::{Curve, DonderTime, Gradient};
use donder_project_io::ProjectSession;

use crate::dto::{
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
