use crate::dto::{
    GuiDocumentRequest, SequenceClipMatchRequest, SequenceClipParam, SequenceClipParamMatch,
    SequenceClipTimeMatch, SequenceParamValue, SequenceSelection,
};
use donder_model::{Effect, FixtureId};
use donder_project_io::ProjectSession;

use super::projection::clip_params;
use super::resolve_request;

/// A clip as matching compares it; an envelope has no `effect` part.
struct MatchClip<'a> {
    id: u32,
    start: f64,
    duration: f64,
    targets: &'a [FixtureId],
    effect: Option<(&'a Effect, Vec<SequenceClipParam>)>,
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
    let sequence = resolved.view::<donder_model::Sequence>(session).object();
    let seconds = |nanos: u128| nanos as f64 / 1e9;
    // Envelopes take part only when asked for.
    let clips: Vec<MatchClip> = sequence
        .composition
        .clips
        .iter()
        .filter(|clip| query.criteria.envelopes || clip.effect().is_some())
        .map(|clip| MatchClip {
            id: clip.id.0,
            start: seconds(super::model::timed(clip).0.as_nanos()),
            duration: seconds(super::model::timed(clip).1.as_nanos()),
            targets: &clip.targets,
            // Parameter values are projected only when a query compares them.
            effect: clip.effect().map(|effect| {
                (
                    effect,
                    if query.params.is_empty() {
                        Vec::new()
                    } else {
                        clip_params(session, sequence, clip, effect)
                    },
                )
            }),
        })
        .collect();
    let seeds: Vec<&MatchClip> = query
        .ids
        .iter()
        .filter_map(|id| {
            let found = clips.iter().find(|clip| clip.id == *id);
            // An envelope seed is skipped when envelopes take no part.
            match found {
                None if !query.criteria.envelopes
                    && sequence
                        .composition
                        .clip(donder_model::ClipId(*id))
                        .is_some_and(|clip| clip.envelope().is_some()) =>
                {
                    None
                }
                found => Some(
                    found
                        .ok_or_else(|| "A selected clip is missing from the sequence.".to_string()),
                ),
            }
        })
        .collect::<Result<_, _>>()?;
    let matching = |clips: &[MatchClip]| -> Vec<u32> {
        clips
            .iter()
            .filter(|candidate| seeds.iter().any(|seed| clips_match(seed, candidate, query)))
            .map(|clip| clip.id)
            .collect()
    };
    Ok(SequenceSelection::Clips {
        ids: matching(&clips),
    })
}

/// Effect type and parameters compare effect clips only: an envelope matches
/// an effect clip only when neither is asked for, and a parameter the seed
/// lacks constrains nothing.
fn clips_match(seed: &MatchClip, candidate: &MatchClip, query: &SequenceClipMatchRequest) -> bool {
    let criteria = &query.criteria;
    let within = |time: SequenceClipTimeMatch, seed: f64, candidate: f64| {
        !time.enabled || (seed - candidate).abs() <= time.tolerance_seconds
    };
    let clip_matches = match (&seed.effect, &candidate.effect) {
        (Some((seed_effect, seed_params)), Some((effect, params))) => {
            (!criteria.effect_type || seed_effect.definition == effect.definition)
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
        && (!criteria.same_lane || seed.targets == candidate.targets)
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
    use SequenceParamValue::{Array, Float, Int};
    match (seed, candidate) {
        (Int { value: seed }, Int { value: candidate })
        | (Float { value: seed }, Float { value: candidate }) => {
            (seed - candidate).abs() <= tolerance
        }
        (Array { values: seed }, Array { values: candidate }) => {
            seed.len() == candidate.len()
                && seed
                    .iter()
                    .zip(candidate)
                    .all(|(seed, candidate)| values_match(seed, candidate, tolerance))
        }
        _ => seed == candidate,
    }
}
