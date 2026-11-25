//! Printed clips kept between prints of a project's documents.
//!
//! A sequence's clips dominate its document, and an edit usually changes one
//! of them. A clip is shared between project snapshots until it is edited, so
//! a clip that is still the allocation it was printed from, in an unchanged
//! printing context, reuses its text.
use super::encode::Encoder;
use crate::ExportProjectError;
use donder_language::data::{Data, DataImport, list_item};
use donder_model::{Clip, ClipId, DonderProject, Layer, MarkCollection, Sequence, SequenceId};
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
    layers: Vec<Layer>,
    marks: Vec<MarkCollection>,
    clips: HashMap<ClipId, (Arc<Clip>, Arc<str>)>,
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
        id: donder_model::SequenceId,
        sequence: &Sequence,
    ) -> Result<Vec<Arc<str>>, ExportProjectError> {
        let cached = self.sequences.entry(id).or_insert_with(|| SequenceClips {
            imports: Vec::new(),
            layers: Vec::new(),
            marks: Vec::new(),
            clips: HashMap::new(),
        });
        if cached.imports != imports
            || cached.layers != sequence.composition.layers
            || cached.marks != sequence.composition.marks
        {
            cached.imports = imports.to_vec();
            cached.layers = sequence.composition.layers.clone();
            cached.marks = sequence.composition.marks.clone();
            cached.clips.clear();
        }
        let mut texts = Vec::with_capacity(sequence.composition.clips.len());
        for clip in &sequence.composition.clips {
            // An envelope's text names what it binds, so it is printed every time.
            if clip.envelope().is_some() {
                texts.push(
                    list_item(
                        &encoder.clip(&sequence.composition, clip)?.encode(),
                        CLIP_LEVEL,
                    )
                    .into(),
                );
                continue;
            }
            let text = match cached.clips.get(&clip.id) {
                Some((printed, text)) if Arc::ptr_eq(printed, clip) => Arc::clone(text),
                _ => {
                    let text: Arc<str> = list_item(
                        &encoder.clip(&sequence.composition, clip)?.encode(),
                        CLIP_LEVEL,
                    )
                    .into();
                    cached
                        .clips
                        .insert(clip.id, (Arc::clone(clip), Arc::clone(&text)));
                    text
                }
            };
            texts.push(text);
        }
        if cached.clips.len() > sequence.composition.clips.len() {
            let current = sequence
                .composition
                .clips
                .iter()
                .map(|clip| &clip.id)
                .collect::<std::collections::HashSet<_>>();
            cached.clips.retain(|id, _| current.contains(id));
        }
        Ok(texts)
    }
}
