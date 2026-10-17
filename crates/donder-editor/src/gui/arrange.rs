use std::collections::{BTreeMap, BTreeSet};

use donder_language::{DonderDuration, DonderTime};
use donder_model::{FixtureTarget, Sequence, SequenceId};
use donder_project_io::ProjectSession;
use donder_sequence_api::{SequenceClipArrangement, SequenceClipRef};

use super::GuiMutationError;
use super::model::{automation_clip_mut, effect_mut};
use super::selection::{lane_targets, nearest_lane};

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

/// Effects and automation clips in one lane are separate rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RowKind {
    Effects,
    Automation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Row {
    lane: usize,
    kind: RowKind,
}

struct Clip {
    clip: SequenceClipRef,
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
    let lanes = lane_targets(session).ok_or_else(|| invalid("Active layout is missing."))?;
    let row = |target: &FixtureTarget, kind| {
        nearest_lane(&lanes, target, 0)
            .map(|lane| Row { lane, kind })
            .ok_or_else(|| invalid("Clip row target is missing."))
    };
    let effects = sequence.effects.iter().map(|effect| {
        Ok(Clip {
            clip: SequenceClipRef::Effect { id: effect.id.0 },
            row: row(&effect.target, RowKind::Effects)?,
            span: Span::of(&effect.start, &effect.duration),
        })
    });
    let automation = sequence.automation_clips.iter().map(|clip| {
        Ok(Clip {
            clip: SequenceClipRef::Automation { id: clip.id.0 },
            row: row(&clip.row_target, RowKind::Automation)?,
            span: Span::of(&clip.start, &clip.duration),
        })
    });
    effects.chain(automation).collect()
}

pub(super) fn arrange_clip_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    effect_ids: &[u32],
    automation_ids: &[u32],
    arrangement: &SequenceClipArrangement,
) -> Result<(), GuiMutationError> {
    let sequence = session
        .project
        .sequence(sequence_id)
        .ok_or_else(|| invalid("Sequence was not found."))?;
    let all = sequence_clips(session, sequence)?;
    let selected: Vec<&Clip> = effect_ids
        .iter()
        .map(|id| SequenceClipRef::Effect { id: *id })
        .chain(
            automation_ids
                .iter()
                .map(|id| SequenceClipRef::Automation { id: *id }),
        )
        .map(|clip| {
            all.iter()
                .find(|candidate| candidate.clip == clip)
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
        match clip.clip {
            SequenceClipRef::Effect { id } => {
                let effect = effect_mut(&mut draft, id)?;
                effect.start = start;
                effect.duration = duration;
            }
            // A clip that keeps its length carries its curve; a resized one
            // crops it, as a resize drag does.
            SequenceClipRef::Automation { id } => {
                let automation = automation_clip_mut(&mut draft, id)?;
                if span.len() == clip.span.len() {
                    automation.start = start;
                } else {
                    automation.crop(start, duration);
                }
            }
        }
    }
    session
        .project
        .replace_sequence(sequence_id, draft)
        .map_err(GuiMutationError::Invalid)?;
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
    let reference_span = |reference: &SequenceClipRef| {
        all.iter()
            .find(|clip| clip.clip == *reference)
            .map(|clip| clip.span)
            .ok_or_else(|| invalid("The reference clip is missing from the sequence."))
    };
    // The reference keeps its timing when it is selected.
    let to_reference = |reference: &SequenceClipRef, place: &dyn Fn(Span, Span) -> Span| {
        let target = reference_span(reference)?;
        Ok(selected
            .iter()
            .map(|clip| {
                if clip.clip == *reference {
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

/// The sorted times of the named mark collections.
fn mark_times(sequence: &Sequence, keys: &[String]) -> Result<Vec<i128>, GuiMutationError> {
    let mut times = Vec::new();
    for key in keys {
        let collection = sequence
            .mark_collections
            .iter()
            .find(|collection| collection.key.name.as_str() == key)
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
