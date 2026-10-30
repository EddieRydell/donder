//! Model start channels, resolved to absolute channels.

use crate::networks::Networks;
use crate::report::UnroutedReason;
use std::collections::HashMap;

/// A start channel as xLights writes it.
#[derive(Debug, PartialEq)]
enum Start {
    /// `N`.
    Absolute(u32),
    /// `>Model:k`: `k` channels after the model's last.
    After(String, u32),
    /// `@Model:k`: the model's `k`th channel.
    At(String, u32),
    /// `#universe:k` or `#ip:universe:k`.
    Universe(Option<String>, u32, u32),
    /// `!Controller:k`.
    Controller(String, u32),
}

fn split_last(text: &str) -> Option<(&str, u32)> {
    let (name, channel) = text.rsplit_once(':')?;
    Some((name.trim(), channel.trim().parse().ok()?))
}

fn parse(text: &str) -> Option<Start> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix('>') {
        let (model, channel) = split_last(rest)?;
        return Some(Start::After(model.to_string(), channel));
    }
    if let Some(rest) = text.strip_prefix('@') {
        let (model, channel) = split_last(rest)?;
        return Some(Start::At(model.to_string(), channel));
    }
    if let Some(rest) = text.strip_prefix('!') {
        let (controller, channel) = split_last(rest)?;
        return Some(Start::Controller(controller.to_string(), channel));
    }
    if let Some(rest) = text.strip_prefix('#') {
        let parts: Vec<&str> = rest.split(':').map(str::trim).collect();
        return match parts[..] {
            [universe, channel] => Some(Start::Universe(
                None,
                universe.parse().ok()?,
                channel.parse().ok()?,
            )),
            [ip, universe, channel] => Some(Start::Universe(
                Some(ip.to_string()),
                universe.parse().ok()?,
                channel.parse().ok()?,
            )),
            _ => None,
        };
    }
    // The old `output:channel` form counts from the show's first channel.
    let channel = text.rsplit_once(':').map_or(text, |(_, channel)| channel);
    channel.trim().parse().ok().map(Start::Absolute)
}

/// What resolving one model needs of it.
pub(crate) struct ModelChannels {
    pub(crate) name: String,
    pub(crate) start: String,
    /// Channels from the model's first to its last, once its start is known.
    pub(crate) span: Option<u32>,
}

enum State {
    Resolving,
    Done(Result<u32, UnroutedReason>),
}

/// Each model's first absolute channel, by its index.
pub(crate) fn resolve(
    models: &[ModelChannels],
    networks: &Networks,
) -> Vec<Result<u32, UnroutedReason>> {
    let by_name: HashMap<&str, usize> = models
        .iter()
        .enumerate()
        .rev()
        .map(|(index, model)| (model.name.as_str(), index))
        .collect();
    let mut states: Vec<Option<State>> = models.iter().map(|_| None).collect();
    (0..models.len())
        .map(|index| first(index, models, networks, &by_name, &mut states))
        .collect()
}

fn first(
    index: usize,
    models: &[ModelChannels],
    networks: &Networks,
    by_name: &HashMap<&str, usize>,
    states: &mut [Option<State>],
) -> Result<u32, UnroutedReason> {
    match &states[index] {
        Some(State::Done(result)) => return result.clone(),
        Some(State::Resolving) => return Err(UnroutedReason::ChainLoop),
        None => {}
    }
    states[index] = Some(State::Resolving);
    let model = &models[index];
    let result = match parse(&model.start) {
        None => Err(UnroutedReason::StartChannel(model.start.clone())),
        Some(Start::Absolute(channel)) => Ok(channel),
        Some(Start::Universe(ip, universe, channel)) => networks
            .universe_start(ip.as_deref(), universe)
            .map(|start| start + channel - 1)
            .ok_or_else(|| UnroutedReason::UnknownUniverse(model.start.clone())),
        Some(Start::Controller(name, channel)) => networks
            .controller_start(&name)
            .map(|start| start + channel - 1)
            .ok_or(UnroutedReason::UnknownController(name)),
        Some(Start::After(other, channel) | Start::At(other, channel)) => {
            let after = matches!(parse(&model.start), Some(Start::After(..)));
            match by_name.get(other.as_str()) {
                None => Err(UnroutedReason::UnknownModel(other)),
                Some(&other_index) => match (
                    first(other_index, models, networks, by_name, states),
                    models[other_index].span,
                ) {
                    (Ok(start), Some(span)) if after => Ok(start + span - 1 + channel),
                    (Ok(start), _) if !after => Ok(start + channel - 1),
                    (Err(UnroutedReason::ChainLoop), _) => Err(UnroutedReason::ChainLoop),
                    _ => Err(UnroutedReason::UnknownModel(other)),
                },
            }
        }
    };
    states[index] = Some(State::Done(result.clone()));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_channel_forms() {
        assert_eq!(parse("12"), Some(Start::Absolute(12)));
        assert_eq!(
            parse(">Left meteor-10:1"),
            Some(Start::After("Left meteor-10".into(), 1))
        );
        assert_eq!(
            parse("#192.168.168.169:232:1"),
            Some(Start::Universe(Some("192.168.168.169".into()), 232, 1))
        );
        assert_eq!(parse("#4:7"), Some(Start::Universe(None, 4, 7)));
        assert_eq!(
            parse("!Garage:31"),
            Some(Start::Controller("Garage".into(), 31))
        );
        assert_eq!(parse("@Star:4"), Some(Start::At("Star".into(), 4)));
    }
}
