use donder_model::{EffectInst, FixtureTarget, SequenceId};
use donder_project_io::ProjectSession;
use donder_sequence_api::{
    DocumentViewId, GuiDocumentRequest, SequenceClipMatchRequest, SequenceClipParamMatch,
    SequenceClipTimeMatch, SequenceEffectParam, SequenceEffectParamValue, SequenceSelection,
};

use super::projection::effect_params;
use super::resolve_request;

/// A clip as matching compares it; automation clips have no effect.
struct MatchClip<'a> {
    id: u32,
    start: f64,
    duration: f64,
    target: &'a FixtureTarget,
    effect: Option<(&'a EffectInst, Vec<SequenceEffectParam>)>,
}

/// "Select Matching": every clip that matches at least one seed clip. Each
/// seed compares against its own values, so close clips never chain.
pub fn matching_sequence_clips(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
    query: &SequenceClipMatchRequest,
) -> Result<SequenceSelection, String> {
    if request.view != DocumentViewId::Sequence {
        return Err("Clip matching belongs to sequence documents.".into());
    }
    let resolved = resolve_request(session, request)?;
    let sequence = session
        .project
        .sequence(&SequenceId(resolved.object_identity()))
        .ok_or("Sequence is not available in the checked project model.")?;
    let seconds = |nanos: u128| nanos as f64 / 1e9;
    let effects: Vec<MatchClip> = sequence
        .effects
        .iter()
        .map(|effect| MatchClip {
            id: effect.id.0,
            start: seconds(effect.start.as_nanos()),
            duration: seconds(effect.duration.as_nanos()),
            target: &effect.target,
            // Parameter values are projected only when a query compares them.
            effect: Some((
                effect,
                if query.params.is_empty() {
                    Vec::new()
                } else {
                    effect_params(session, sequence, effect)
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
                effect: None,
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
    // Automation seeds take part only when automation clips do.
    let seeds: Vec<&MatchClip> = seed(&effects, &query.effect_ids)?
        .into_iter()
        .map(|index| &effects[index])
        .chain(
            seed(
                &automation,
                if query.criteria.automation_clips {
                    &query.automation_ids
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
        effect_ids: matching(&effects),
        automation_ids: matching(&automation),
    })
}

/// Effect type and parameters compare effects only: an automation clip
/// matches an effect only when neither is asked for, and a parameter the seed
/// lacks constrains nothing.
fn clips_match(seed: &MatchClip, candidate: &MatchClip, query: &SequenceClipMatchRequest) -> bool {
    let criteria = &query.criteria;
    let within = |time: SequenceClipTimeMatch, seed: f64, candidate: f64| {
        !time.enabled || (seed - candidate).abs() <= time.tolerance_seconds
    };
    let effect_matches = match (&seed.effect, &candidate.effect) {
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
    effect_matches
        && within(criteria.start, seed.start, candidate.start)
        && within(
            criteria.end,
            seed.start + seed.duration,
            candidate.start + candidate.duration,
        )
        && within(criteria.duration, seed.duration, candidate.duration)
        && (!criteria.same_lane || seed.target == candidate.target)
}

fn param_value<'a>(
    params: &'a [SequenceEffectParam],
    name: &str,
) -> Option<&'a SequenceEffectParamValue> {
    params
        .iter()
        .find(|param| param.name == name)
        .map(|param| &param.value)
}

fn param_matches(
    seed: &[SequenceEffectParam],
    candidate: &[SequenceEffectParam],
    param: &SequenceClipParamMatch,
) -> bool {
    let Some(seed) = param_value(seed, &param.name) else {
        return true;
    };
    param_value(candidate, &param.name)
        .is_some_and(|candidate| values_match(seed, candidate, param.tolerance))
}

/// Numbers match within `tolerance`; every other value matches exactly.
fn values_match(
    seed: &SequenceEffectParamValue,
    candidate: &SequenceEffectParamValue,
    tolerance: f32,
) -> bool {
    use SequenceEffectParamValue::{Float, FloatArray, Int, IntArray};
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
