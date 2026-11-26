use std::collections::{BTreeMap, BTreeSet};

use crate::dto::SequenceClipArrangement;
use donder_language::{DonderDuration, DonderTime};
use donder_model::{FixtureId, Sequence};
use donder_project_io::ProjectSession;

use super::model::{clip_mut, timed};
use super::selection::{lane_targets, nearest_lane};
use super::{GuiMutationError, ResolvedGuiObject};

/// A clip's timing in whole nanoseconds, so aligned edges are exactly equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    start: i128,
    end: i128,
}

impl Span {
    fn of(start: &DonderTime, duration: &DonderDuration) -> Self {
        let start = start.as_nanos() as i128;
        Self {
            start,
            end: start + duration.as_nanos() as i128,
        }
    }

    fn len(self) -> i128 {
        self.end - self.start
    }

    fn moved_to(self, start: i128) -> Self {
        Self {
            start,
            end: start + self.len(),
        }
    }
}

/// Effect clips and envelopes in one lane are separate rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RowKind {
    Clips,
    Envelopes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Row {
    lane: usize,
    kind: RowKind,
}

struct Clip {
    id: u32,
    row: Row,
    span: Span,
}

fn invalid(message: &str) -> GuiMutationError {
    GuiMutationError::Invalid(message.into())
}

/// Every clip of the sequence with its row, in sequence order.
fn sequence_clips(
    session: &ProjectSession,
    sequence: &Sequence,
) -> Result<Vec<Clip>, GuiMutationError> {
    let lanes = lane_targets(session);
    let row = |target: &FixtureId, kind| {
        nearest_lane(&lanes, target, 0)
            .map(|lane| Row { lane, kind })
            .ok_or_else(|| invalid("Clip row target is missing."))
    };
    sequence
        .composition
        .clips
        .iter()
        .map(|clip| {
            let kind = match clip.envelope() {
                Some(_) => RowKind::Envelopes,
                None => RowKind::Clips,
            };
            Ok(Clip {
                id: clip.id.0,
                // A clip of several targets arranges in its first target's row.
                row: row(&clip.targets[0], kind)?,
                span: {
                    let (start, duration) = timed(clip);
                    Span::of(start, duration)
                },
            })
        })
        .collect()
}

pub(super) fn arrange_clip_selection(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    ids: &[u32],
    arrangement: &SequenceClipArrangement,
) -> Result<(), GuiMutationError> {
    let sequence = resolved.view::<Sequence>(session).object();
    let all = sequence_clips(session, sequence)?;
    let selected: Vec<&Clip> = ids
        .iter()
        .map(|id| {
            all.iter()
                .find(|candidate| candidate.id == *id)
                .ok_or_else(|| invalid("A selected clip is missing from the sequence."))
        })
        .collect::<Result<_, _>>()?;
    let spans = arranged(sequence, &all, &selected, arrangement)?;
    let mut draft = sequence.clone();
    for (clip, span) in selected.iter().zip(spans) {
        if span == clip.span {
            continue;
        }
        if span.start < 0 {
            return Err(invalid("A clip would start before the sequence."));
        }
        if span.len() <= 0 {
            return Err(invalid("A clip would have no length."));
        }
        let start = DonderTime::from_nanos(
            u64::try_from(span.start).map_err(|_| invalid("A clip start is out of range."))?,
        );
        let duration = DonderDuration::from_nanos(
            u64::try_from(span.len()).map_err(|_| invalid("A clip length is out of range."))?,
        );
        // A clip that keeps its length carries an envelope's curve; a
        // resized one crops it, as a resize drag does.
        let authored = clip_mut(&mut draft, clip.id)?;
        if span.len() == clip.span.len() {
            authored.placement = donder_model::Placement::Timed { start, duration };
        } else {
            authored.crop(start, duration);
        }
    }
    session
        .put((resolved.id(), draft))
        .map_err(GuiMutationError::from)?;
    Ok(())
}

/// The arranged span of each selected clip, in selection order.
fn arranged(
    sequence: &Sequence,
    all: &[Clip],
    selected: &[&Clip],
    arrangement: &SequenceClipArrangement,
) -> Result<Vec<Span>, GuiMutationError> {
    use SequenceClipArrangement as A;
    let reference_span = |reference: &u32| {
        all.iter()
            .find(|clip| clip.id == *reference)
            .map(|clip| clip.span)
            .ok_or_else(|| invalid("The reference clip is missing from the sequence."))
    };
    // The reference keeps its timing when it is selected.
    let to_reference = |reference: &u32, place: &dyn Fn(Span, Span) -> Span| {
        let target = reference_span(reference)?;
        Ok(selected
            .iter()
            .map(|clip| {
                if clip.id == *reference {
                    clip.span
                } else {
                    place(clip.span, target)
                }
            })
            .collect())
    };
    let each =
        |place: &dyn Fn(Span) -> Span| selected.iter().map(|clip| place(clip.span)).collect();
    match arrangement {
        A::AlignStarts { reference } => to_reference(reference, &|clip, target| Span {
            start: target.start,
            end: clip.end,
        }),
        A::AlignEnds { reference } => to_reference(reference, &|clip, target| Span {
            start: clip.start,
            end: target.end,
        }),
        A::MoveStarts { reference } => {
            to_reference(reference, &|clip, target| clip.moved_to(target.start))
        }
        A::MoveEnds { reference } => to_reference(reference, &|clip, target| {
            clip.moved_to(target.end - clip.len())
        }),
        A::AlignBoth { reference } => to_reference(reference, &|_, target| target),
        A::AlignCenters { reference } => to_reference(reference, &|clip, target| {
            clip.moved_to((target.start + target.end) / 2 - clip.len() / 2)
        }),
        A::MatchDuration { reference } => to_reference(reference, &|clip, target| Span {
            start: clip.start,
            end: clip.start + target.len(),
        }),
        A::StartAfter { reference } => {
            to_reference(reference, &|clip, target| clip.moved_to(target.end))
        }
        A::EndBefore { reference } => to_reference(reference, &|clip, target| {
            clip.moved_to(target.start - clip.len())
        }),
        A::QuantizeToMarks { mark_collections } => {
            let marks = mark_times(sequence, mark_collections)?;
            let nearest = |time: i128, after: i128| {
                marks
                    .iter()
                    .copied()
                    .filter(|mark| *mark > after)
                    .min_by_key(|mark| (mark - time).abs())
            };
            Ok(each(&|clip| {
                let start = nearest(clip.start, i128::MIN).unwrap_or(clip.start);
                let end = nearest(clip.end, start).unwrap_or(start + clip.len());
                Span { start, end }
            }))
        }
        A::StretchToMarks { mark_collections } => {
            let marks = mark_times(sequence, mark_collections)?;
            Ok(each(&|clip| Span {
                start: marks
                    .iter()
                    .copied()
                    .filter(|mark| *mark <= clip.start)
                    .max()
                    .unwrap_or(clip.start),
                end: marks
                    .iter()
                    .copied()
                    .filter(|mark| *mark >= clip.end)
                    .min()
                    .unwrap_or(clip.end),
            }))
        }
        A::FillGaps => Ok(selected
            .iter()
            .map(|clip| {
                let next = all
                    .iter()
                    .filter(|other| other.row == clip.row && other.span.start > clip.span.start)
                    .map(|other| other.span.start)
                    .min();
                match next {
                    Some(next) if next > clip.span.end => Span {
                        start: clip.span.start,
                        end: next,
                    },
                    _ => clip.span,
                }
            })
            .collect()),
        A::ButtTogether => by_row(selected, |row| {
            let mut cursor = row[0].end;
            row.iter()
                .enumerate()
                .map(|(index, span)| {
                    if index == 0 {
                        return Ok(*span);
                    }
                    let moved = span.moved_to(cursor);
                    cursor = moved.end;
                    Ok(moved)
                })
                .collect()
        }),
        A::DistributeEvenly => by_row(selected, |row| {
            let (first, last) = (row[0], row[row.len() - 1]);
            let lengths: i128 = row.iter().map(|span| span.len()).sum();
            let gaps = last.end - first.start - lengths;
            if row.len() > 2 && gaps < 0 {
                return Err(invalid(
                    "The clips in a row are longer than the span they distribute over.",
                ));
            }
            let mut before = 0;
            row.iter()
                .enumerate()
                .map(|(index, span)| {
                    let placed = if index == 0 || index == row.len() - 1 {
                        *span
                    } else {
                        span.moved_to(
                            first.start + before + gaps * index as i128 / (row.len() as i128 - 1),
                        )
                    };
                    before += span.len();
                    Ok(placed)
                })
                .collect()
        }),
        A::Cascade {
            step_seconds,
            reverse,
        } => {
            if !step_seconds.is_finite() {
                return Err(invalid("The cascade step must be finite."));
            }
            let step = (*step_seconds * 1e9).round() as i128;
            let lanes: BTreeSet<usize> = selected.iter().map(|clip| clip.row.lane).collect();
            let ranks: BTreeMap<usize, usize> = lanes
                .iter()
                .enumerate()
                .map(|(rank, lane)| {
                    (
                        *lane,
                        if *reverse {
                            lanes.len() - 1 - rank
                        } else {
                            rank
                        },
                    )
                })
                .collect();
            Ok(selected
                .iter()
                .map(|clip| {
                    clip.span
                        .moved_to(clip.span.start + ranks[&clip.row.lane] as i128 * step)
                })
                .collect())
        }
        A::EqualizeDurations => {
            let average = selected.iter().map(|clip| clip.span.len()).sum::<i128>()
                / selected.len().max(1) as i128;
            Ok(each(&|clip| Span {
                start: clip.start,
                end: clip.start + average,
            }))
        }
    }
}

/// Arranges each row's selected clips in start order with `arrange`, which
/// receives a nonempty row.
fn by_row(
    selected: &[&Clip],
    arrange: impl Fn(&[Span]) -> Result<Vec<Span>, GuiMutationError>,
) -> Result<Vec<Span>, GuiMutationError> {
    let mut rows: BTreeMap<Row, Vec<usize>> = BTreeMap::new();
    for (index, clip) in selected.iter().enumerate() {
        rows.entry(clip.row).or_default().push(index);
    }
    let mut spans: Vec<Span> = selected.iter().map(|clip| clip.span).collect();
    for indexes in rows.values_mut() {
        indexes.sort_by_key(|index| selected[*index].span.start);
        let row: Vec<Span> = indexes.iter().map(|index| selected[*index].span).collect();
        for (index, span) in indexes.iter().zip(arrange(&row)?) {
            spans[*index] = span;
        }
    }
    Ok(spans)
}

/// The sorted times of the mark collections `ids`.
fn mark_times(sequence: &Sequence, ids: &[u32]) -> Result<Vec<i128>, GuiMutationError> {
    let mut times = Vec::new();
    for id in ids {
        let collection = sequence
            .composition
            .marks
            .iter()
            .find(|collection| collection.id.0 == *id)
            .ok_or_else(|| invalid("Mark collection was not found."))?;
        times.extend(
            collection
                .marks
                .iter()
                .map(|mark| mark.time.as_nanos() as i128),
        );
    }
    if times.is_empty() {
        return Err(invalid(
            "Show a mark collection with marks to arrange clips to marks.",
        ));
    }
    times.sort_unstable();
    Ok(times)
}
