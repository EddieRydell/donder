use crate::dto::{
    GuiDocumentRequest, SequenceClipMatchRequest, SequenceClipParam, SequenceClipParamMatch,
    SequenceClipTimeMatch, SequenceParamValue, SequenceSelection,
};
use donder_model::{Clip, FixtureTarget, SequenceId};
use donder_project_io::ProjectSession;

use super::projection::clip_params;
use super::resolve_request;

/// A clip or automation clip as matching compares it; an automation clip has
/// no `clip` part.
struct MatchClip<'a> {
    id: u32,
    start: f64,
    duration: f64,
    target: &'a FixtureTarget,
    clip: Option<(&'a Clip, Vec<SequenceClipParam>)>,
}

/// "Select Matching": every clip that matches at least one seed clip. Each
/// seed compares against its own values, so close clips never chain.
pub fn matching_sequence_clips(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
    query: &SequenceClipMatchRequest,
) -> Result<SequenceSelection, String> {
    let resolved = resolve_request(session, request)?;
    if resolved.kind != donder_model::ObjectKind::Sequence {
        return Err("Clip matching belongs to sequence documents.".into());
    }
    let sequence = session
        .project
        .get::<donder_model::Sequence>(resolved.id::<SequenceId>())
        .ok_or("Sequence is not available in the checked project model.")?;
    let seconds = |nanos: u128| nanos as f64 / 1e9;
    let clips: Vec<MatchClip> = sequence
        .clips
        .iter()
        .map(|clip| MatchClip {
            id: clip.id.0,
            start: seconds(clip.start.as_nanos()),
            duration: seconds(clip.duration.as_nanos()),
            target: &clip.target,
            // Parameter values are projected only when a query compares them.
            clip: Some((
                clip,
                if query.params.is_empty() {
                    Vec::new()
                } else {
                    clip_params(session, sequence, clip)
                },
            )),
        })
        .collect();
    let automation: Vec<MatchClip> = if query.criteria.automation_clips {
        sequence
            .automation_clips
            .iter()
            .map(|clip| MatchClip {
                id: clip.id.0,
                start: seconds(clip.start.as_nanos()),
                duration: seconds(clip.duration.as_nanos()),
                target: &clip.row_target,
                clip: None,
            })
            .collect()
    } else {
        Vec::new()
    };
    let seed = |clips: &'_ [MatchClip], ids: &[u32]| -> Result<Vec<usize>, String> {
        ids.iter()
            .map(|id| {
                clips
                    .iter()
                    .position(|clip| clip.id == *id)
                    .ok_or_else(|| "A selected clip is missing from the sequence.".to_string())
            })
            .collect()
    };
    let (clip_ids, automation_ids) = super::selection::split_clips(&query.clips);
    // Automation seeds take part only when automation clips do.
    let seeds: Vec<&MatchClip> = seed(&clips, &clip_ids)?
        .into_iter()
        .map(|index| &clips[index])
        .chain(
            seed(
                &automation,
                if query.criteria.automation_clips {
                    &automation_ids
                } else {
                    &[]
                },
            )?
            .into_iter()
            .map(|index| &automation[index]),
        )
        .collect();
    let matching = |clips: &[MatchClip]| -> Vec<u32> {
        clips
            .iter()
            .filter(|candidate| seeds.iter().any(|seed| clips_match(seed, candidate, query)))
            .map(|clip| clip.id)
            .collect()
    };
    Ok(SequenceSelection::Clips {
        clips: super::selection::clip_refs(&matching(&clips), &matching(&automation)),
    })
}

/// Effect type and parameters compare clips only: an automation clip
/// matches a clip only when neither is asked for, and a parameter the seed
/// lacks constrains nothing.
fn clips_match(seed: &MatchClip, candidate: &MatchClip, query: &SequenceClipMatchRequest) -> bool {
    let criteria = &query.criteria;
    let within = |time: SequenceClipTimeMatch, seed: f64, candidate: f64| {
        !time.enabled || (seed - candidate).abs() <= time.tolerance_seconds
    };
    let clip_matches = match (&seed.clip, &candidate.clip) {
        (Some((seed_clip, seed_params)), Some((clip, params))) => {
            (!criteria.effect_type || seed_clip.definition == clip.definition)
                && query
                    .params
                    .iter()
                    .all(|param| param_matches(seed_params, params, param))
        }
        (Some((_, seed_params)), None) => {
            !criteria.effect_type
                && query
                    .params
                    .iter()
                    .all(|param| param_value(seed_params, &param.name).is_none())
        }
        (None, Some(_)) => !criteria.effect_type,
        (None, None) => true,
    };
    clip_matches
        && within(criteria.start, seed.start, candidate.start)
        && within(
            criteria.end,
            seed.start + seed.duration,
            candidate.start + candidate.duration,
        )
        && within(criteria.duration, seed.duration, candidate.duration)
        && (!criteria.same_lane || seed.target == candidate.target)
}

fn param_value<'a>(params: &'a [SequenceClipParam], name: &str) -> Option<&'a SequenceParamValue> {
    params
        .iter()
        .find(|param| param.name == name)
        .map(|param| &param.value)
}

fn param_matches(
    seed: &[SequenceClipParam],
    candidate: &[SequenceClipParam],
    param: &SequenceClipParamMatch,
) -> bool {
    let Some(seed) = param_value(seed, &param.name) else {
        return true;
    };
    param_value(candidate, &param.name)
        .is_some_and(|candidate| values_match(seed, candidate, param.tolerance))
}

/// Numbers match within `tolerance`; every other value matches exactly.
fn values_match(seed: &SequenceParamValue, candidate: &SequenceParamValue, tolerance: f32) -> bool {
    use SequenceParamValue::{Float, FloatArray, Int, IntArray};
    let near = |seed: f32, candidate: f32| (seed - candidate).abs() <= tolerance;
    match (seed, candidate) {
        (Int { value: seed }, Int { value: candidate })
        | (Float { value: seed }, Float { value: candidate }) => near(*seed, *candidate),
        (IntArray { values: seed }, IntArray { values: candidate })
        | (FloatArray { values: seed }, FloatArray { values: candidate }) => {
            seed.len() == candidate.len()
                && seed
                    .iter()
                    .zip(candidate)
                    .all(|(seed, candidate)| near(*seed, *candidate))
        }
        _ => seed == candidate,
    }
}
