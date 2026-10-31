//! Model start channels, resolved to absolute channels as xLights resolves
//! them when it loads a show.

use crate::report::UnroutedReason;
use crate::xlights::networks::Networks;
use crate::xlights::read::{strtol, trim};
use std::collections::{BTreeMap, HashSet};

/// xLights: `NO_CONTROLLER`, a model's `Controller` that takes it off every
/// controller.
const NO_CONTROLLER: &str = "No Controller";

/// What resolving one model needs of it.
pub(crate) struct ModelChannels {
    pub(crate) name: String,
    /// The `StartChannel` as written.
    pub(crate) start: String,
    /// The model's `Controller`.
    pub(crate) controller: String,
    /// The channels the model takes after its start, when they are known.
    pub(crate) span: Option<crate::xlights::models::Span>,
}

/// A model's resolved first channel, from 1, or why xLights cannot compute
/// it.
type Resolved = Result<u32, UnroutedReason>;

/// The first character of a start channel, as `RecalcStartChannels` sorts
/// models by it.
fn first_char(start: &str) -> char {
    trim(start).chars().next().unwrap_or('0')
}

struct State<'a> {
    models: &'a [ModelChannels],
    groups: &'a HashSet<String>,
    by_name: BTreeMap<&'a str, usize>,
    networks: &'a Networks,
    resolved: Vec<Option<Resolved>>,
}

impl State<'_> {
    /// xLights: `CouldComputeStartChannel`.
    fn could(&self, index: usize) -> bool {
        matches!(self.resolved[index], Some(Ok(_)))
    }

    /// The last channel of a resolved model, from 0, when its span is known.
    /// xLights: `Model::GetLastChannel`.
    fn last(&self, index: usize) -> Option<i64> {
        let first = self.resolved[index].clone()?.ok()?;
        let span = self.models[index].span?;
        Some(i64::from(first) - 1 + span.end as i64 - 1)
    }

    /// xLights: `Model::GetNumberFromChannelString`, as `UpdateChannels`
    /// calls it for the model `index`: its first channel, or why it is
    /// invalid.
    fn number(&self, index: usize) -> Resolved {
        let model = &self.models[index];
        let text = model.start.as_str();
        let invalid = || UnroutedReason::StartChannel(text.to_string());
        let checked = |value: i64| {
            u32::try_from(value)
                .ok()
                .filter(|value| *value >= 1)
                .ok_or_else(invalid)
        };
        let sc = trim(text);
        if let Some(colon) = sc.find(':') {
            let start = &sc[..colon];
            let rest = &sc[colon + 1..];
            match start.chars().next() {
                Some(kind @ ('@' | '<' | '>')) => {
                    let channel = strtol(rest) as i32;
                    let name = trim(&start[1..]);
                    let Some(&other) = self.by_name.get(name) else {
                        // xLights: a group is in the same table, but its
                        // start channel is never computed.
                        if self.groups.contains(name) {
                            return Err(UnroutedReason::FollowsGroup(name.to_string()));
                        }
                        return Err(UnroutedReason::UnknownModel(name.to_string()));
                    };
                    if other == index && !self.could(index) {
                        return Err(UnroutedReason::ChainLoop);
                    }
                    if !self.could(other) {
                        return Err(match &self.resolved[other] {
                            Some(Err(reason)) => UnroutedReason::Follows {
                                model: name.to_string(),
                                reason: Box::new(reason.clone()),
                            },
                            _ => UnroutedReason::ChainOrder(name.to_string()),
                        });
                    }
                    if kind == '>' && self.models[other].controller == NO_CONTROLLER {
                        return Err(UnroutedReason::NoController(name.to_string()));
                    }
                    let value = if kind == '@' {
                        // xLights: Model::GetFirstChannel, the model's lowest
                        // node channel.
                        let first = self.resolved[other].clone().and_then(Result::ok);
                        first.zip(self.models[other].span).map(|(first, span)| {
                            i64::from(first) - 1 + span.first as i64 + i64::from(channel)
                        })
                    } else {
                        self.last(other).map(|last| last + i64::from(channel) + 1)
                    };
                    let value =
                        value.ok_or_else(|| UnroutedReason::UnknownSpan(name.to_string()))?;
                    return checked(value);
                }
                Some('!') => {
                    if !rest.contains(':') {
                        let name = trim(&start[1..]);
                        let first = self
                            .networks
                            .controller_start(name)
                            .ok_or_else(|| UnroutedReason::UnknownController(name.to_string()))?;
                        return checked(i64::from(first) - 1 + i64::from(strtol(rest) as i32));
                    }
                    return Err(invalid());
                }
                Some('#') => {
                    // xLights splits the start channel as written, less its
                    // first character.
                    let parts: Vec<&str> = text.get(1..).unwrap_or_default().split(':').collect();
                    let (ip, universe, channel) = match parts[..] {
                        [ip, universe, channel] => {
                            (trim(ip), strtol(universe) as i32, strtol(channel) as i32)
                        }
                        [universe, _] => ("", strtol(universe) as i32, strtol(rest) as i32),
                        _ => return Err(invalid()),
                    };
                    let first = self
                        .networks
                        .universe_start(ip, universe)
                        .ok_or_else(|| UnroutedReason::UnknownUniverse(text.to_string()))?;
                    return checked(i64::from(first) + i64::from(channel) - 1);
                }
                _ => {}
            }
            return checked(i64::from(strtol(rest) as i32));
        }
        checked(i64::from(strtol(sc) as i32))
    }

    /// xLights: `Model::UpdateChannels`. A model on "No Controller" keeps
    /// its start channel: loading sets the controller, which clears it,
    /// then the start channel again.
    fn update(&mut self, index: usize) {
        self.resolved[index] = Some(self.number(index));
    }
}

/// Each model's first absolute channel, by its index. xLights:
/// `ModelManager::RecalcStartChannels`, which resolves models in name
/// order: first those that do not follow another, then those following a
/// resolved model, then the rest.
pub(crate) fn resolve(
    models: &[ModelChannels],
    groups: &HashSet<String>,
    networks: &Networks,
) -> Vec<Resolved> {
    let mut by_name = BTreeMap::new();
    for (index, model) in models.iter().enumerate() {
        by_name.entry(model.name.as_str()).or_insert(index);
    }
    let order: Vec<usize> = by_name.values().copied().collect();
    let mut state = State {
        models,
        groups,
        by_name,
        networks,
        resolved: vec![None; models.len()],
    };
    // xLights loads models in parallel, so of models sharing a name it keeps
    // either; none of them resolves.
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for model in models {
        *counts.entry(model.name.as_str()).or_default() += 1;
    }
    let duplicated = |index: usize| counts[models[index].name.as_str()] > 1;
    for (index, model) in models.iter().enumerate() {
        if duplicated(index) {
            state.resolved[index] = Some(Err(UnroutedReason::Duplicate(model.name.clone())));
        }
    }
    let mut done = HashSet::new();
    let mut on_no_controller = HashSet::new();
    for &index in &order {
        let model = &models[index];
        if duplicated(index) {
            continue;
        }
        if !matches!(first_char(&model.start), '>' | '@') {
            done.insert(model.name.as_str());
            state.update(index);
        }
        if model.controller == NO_CONTROLLER {
            on_no_controller.insert(model.name.as_str());
        }
    }
    loop {
        let mut progress = false;
        for &index in &order {
            let model = &models[index];
            let first = first_char(&model.start);
            if !matches!(first, '>' | '@') || state.could(index) || duplicated(index) {
                continue;
            }
            let start = trim(&model.start);
            let end = start.find(':').unwrap_or(start.len()).max(1);
            let depends = trim(&start[1..end]);
            if first == '>' && on_no_controller.contains(depends) {
                on_no_controller.insert(model.name.as_str());
            }
            if done.contains(depends) && !on_no_controller.contains(depends) {
                done.insert(model.name.as_str());
                state.update(index);
                if state.could(index) {
                    progress = true;
                }
            }
        }
        if !progress {
            break;
        }
    }
    for &index in &order {
        if matches!(first_char(&models[index].start), '>' | '@')
            && !state.could(index)
            && !duplicated(index)
        {
            state.update(index);
        }
    }
    state
        .resolved
        .into_iter()
        .enumerate()
        .map(|(index, resolved)| {
            resolved.unwrap_or_else(|| Err(UnroutedReason::Duplicate(models[index].name.clone())))
        })
        .collect()
}
