//! Desktop adapter for sequence-clip rasters.
//!
//! Every clip of the open sequence gets a raster, rendered once at its natural
//! resolution by one background worker, clips on screen first. A clip that is
//! still the allocation of the previous project snapshot, in an unchanged
//! context, keeps its raster, so an edit renders only the clips it changed.
//! Effect evaluation remains in `donder-runtime`; scaling and drawing remain
//! in the frontend.

mod render;
mod service;
mod worker;

pub(crate) use service::SequenceClipRasterService;
