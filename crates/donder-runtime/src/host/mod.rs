//! Host preparation: shows built from accepted inputs, and their authored
//! clips resolved for rasters.
use donder_runtime_types::{SequenceTiming, Shared};

use crate::show::PreparedShow;

mod builder;
mod clip;

pub use builder::{ClipHandle, OutputHandle, ShowBuilder, SignalHandle, TargetHandle};
pub use clip::{ClipRaster, ShowClip};

impl PreparedShow {
    /// Lower accepted inputs through owner-branded handles. All graph addresses,
    /// execution slots, and output spans are derived by the builder.
    pub fn build(
        timing: SequenceTiming,
        build: impl for<'id> FnOnce(&mut ShowBuilder<'id>) -> Option<SignalHandle<'id>>,
    ) -> Self {
        let mut builder = ShowBuilder::new(timing);
        let root = build(&mut builder);
        let words: Shared<[u32]> = builder.finish(root).into();
        Self::trusted(words)
    }

    /// Resolve an authored clip without loading or preparing its source again.
    pub fn clip(&self, id: u32) -> Option<ShowClip<'_>> {
        let show = self.show();
        let index = show
            .s
            .clip_index
            .binary_search_by_key(&id, |entry| entry.id)
            .ok()?;
        Some(ShowClip::new(show, show.s.clip_index[index].clip as usize))
    }
}
