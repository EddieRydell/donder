//! Printed clips kept between prints of a project's documents.
//!
//! A sequence's clips dominate its document, and an edit usually changes one
//! of them. A clip is shared between project snapshots until it is edited, so
//! a clip that is still the allocation it was printed from, in an unchanged
//! printing context, reuses its text.
use super::encode::Encoder;
use crate::ExportProjectError;
use donder_language::data::{Data, DataImport, list_item};
use donder_model::{
    DonderProject, EffectInst, EffectInstId, MarkCollection, Sequence, SequenceId, SequenceLayer,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Clips are items of the `clips` field, one level inside their declaration.
const CLIP_LEVEL: usize = 2;

#[derive(Default)]
pub struct DocumentTextCache {
    /// The project the cached clips were printed in.
    project: Option<DonderProject>,
    sequences: HashMap<SequenceId, SequenceClips>,
}

/// The printing context of one sequence's clips besides the project's stores.
struct SequenceClips {
    imports: Vec<DataImport>,
    layers: Vec<SequenceLayer>,
    marks: Vec<MarkCollection>,
    clips: HashMap<EffectInstId, (Arc<EffectInst>, Arc<str>)>,
}

impl DocumentTextCache {
    /// Forget everything when a store other than sequences changed: layouts
    /// and definitions decide how clips name their targets and parameters.
    pub(crate) fn begin(&mut self, project: &DonderProject) {
        if !self
            .project
            .as_ref()
            .is_some_and(|cached| project.only_sequences_differ_from(cached))
        {
            self.sequences.clear();
        }
        self.project = Some(project.clone());
    }

    /// The text of each of `sequence`'s clips, in order, as list items of its
    /// declaration in a document with `imports`.
    pub(crate) fn clips(
        &mut self,
        encoder: &Encoder<'_>,
        imports: &[DataImport],
        sequence: &Sequence,
    ) -> Result<Vec<Arc<str>>, ExportProjectError> {
        let cached = self
            .sequences
            .entry(sequence.id.clone())
            .or_insert_with(|| SequenceClips {
                imports: Vec::new(),
                layers: Vec::new(),
                marks: Vec::new(),
                clips: HashMap::new(),
            });
        if cached.imports != imports
            || cached.layers != sequence.layers
            || cached.marks != sequence.mark_collections
        {
            cached.imports = imports.to_vec();
            cached.layers = sequence.layers.clone();
            cached.marks = sequence.mark_collections.clone();
            cached.clips.clear();
        }
        let mut texts = Vec::with_capacity(sequence.effects.len());
        for effect in &sequence.effects {
            let text = match cached.clips.get(&effect.id) {
                Some((printed, text)) if Arc::ptr_eq(printed, effect) => Arc::clone(text),
                _ => {
                    let text: Arc<str> =
                        list_item(&encoder.clip(sequence, effect)?.encode(), CLIP_LEVEL).into();
                    cached
                        .clips
                        .insert(effect.id.clone(), (Arc::clone(effect), Arc::clone(&text)));
                    text
                }
            };
            texts.push(text);
        }
        if cached.clips.len() > sequence.effects.len() {
            let current = sequence
                .effects
                .iter()
                .map(|effect| &effect.id)
                .collect::<std::collections::HashSet<_>>();
            cached.clips.retain(|id, _| current.contains(id));
        }
        Ok(texts)
    }
}
