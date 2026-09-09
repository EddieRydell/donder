use super::{ControlTarget, ControlTargetError};
use crate::element::{ElementNode, ElementNodeKind, ElementSelection, ElementTree, IndexedOption};
use crate::fixture_profile::{
    FixtureFunctionId, FixtureFunctionKind, FixtureIndexedEntry, FixtureProfileStore,
};

#[derive(Clone, Debug)]
pub enum ControlChannelOptions {
    Normalized,
    Indexed(Vec<IndexedOption>),
    FixtureIndexed(Vec<FixtureIndexedEntry>),
    Color,
}

pub struct ControlChannel {
    pub target: ControlTarget,
    pub label: String,
    pub cell_count: Option<u32>,
    pub options: ControlChannelOptions,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChannelKind {
    Scalar,
    Indexed,
    Fixture(FixtureFunctionId),
}

struct NodeChannel {
    kind: ChannelKind,
    name: String,
    options: ControlChannelOptions,
}

fn node_channels(
    node: &ElementNode,
    profiles: &FixtureProfileStore,
) -> Result<Vec<NodeChannel>, ControlTargetError> {
    Ok(match &node.kind {
        ElementNodeKind::Scalar { .. } => vec![NodeChannel {
            kind: ChannelKind::Scalar,
            name: "Level".into(),
            options: ControlChannelOptions::Normalized,
        }],
        ElementNodeKind::Indexed { options, .. } => vec![NodeChannel {
            kind: ChannelKind::Indexed,
            name: "Option".into(),
            options: ControlChannelOptions::Indexed(options.clone()),
        }],
        ElementNodeKind::Fixture { profile } => profiles
            .definitions
            .get(profile)
            .ok_or_else(|| ControlTargetError::MissingProfile(profile.clone()))?
            .functions
            .iter()
            .map(|(id, function)| NodeChannel {
                kind: ChannelKind::Fixture(*id),
                name: format!("{} (function {})", function.name, id.0),
                options: match &function.kind {
                    FixtureFunctionKind::Range => ControlChannelOptions::Normalized,
                    FixtureFunctionKind::Indexed { entries }
                    | FixtureFunctionKind::ColorWheel { entries } => {
                        ControlChannelOptions::FixtureIndexed(entries.clone())
                    }
                    FixtureFunctionKind::ColorMixing { .. } => ControlChannelOptions::Color,
                },
            })
            .collect(),
        ElementNodeKind::Group { .. } | ElementNodeKind::Color { .. } => Vec::new(),
    })
}

impl ControlChannelOptions {
    fn intersect(&mut self, other: &Self) -> bool {
        match (self, other) {
            (Self::Normalized, Self::Normalized) | (Self::Color, Self::Color) => true,
            (Self::Indexed(options), Self::Indexed(other)) => {
                options.retain(|option| other.iter().any(|candidate| candidate.id == option.id));
                !options.is_empty()
            }
            (Self::FixtureIndexed(entries), Self::FixtureIndexed(other)) => {
                entries.retain_mut(|entry| {
                    if let Some(candidate) = other.iter().find(|candidate| candidate.id == entry.id)
                    {
                        entry.curve_control &= candidate.curve_control;
                        true
                    } else {
                        false
                    }
                });
                !entries.is_empty()
            }
            _ => false,
        }
    }
}

/// Describe controls supported by every selected leaf. Group options are the
/// intersection of their members' option/entry IDs, preserving authored order.
pub fn control_channels(
    tree: &ElementTree,
    profiles: &FixtureProfileStore,
) -> Result<Vec<ControlChannel>, ControlTargetError> {
    let mut result = Vec::new();
    for (id, node) in &tree.nodes {
        let selection = ElementSelection {
            tree: tree.id.clone(),
            node: *id,
            cells: None,
        };
        let addresses = tree
            .flatten_selection(&selection)
            .map_err(ControlTargetError::Selection)?;
        let mut members = indexmap::IndexSet::new();
        for address in addresses {
            members.insert(address.node);
        }
        let mut members = members.into_iter();
        let Some(first) = members.next() else {
            continue;
        };
        let mut channels = node_channels(&tree.nodes[&first], profiles)?;
        for member in members {
            let other = node_channels(&tree.nodes[&member], profiles)?;
            channels.retain_mut(|channel| {
                other
                    .iter()
                    .find(|other| other.kind == channel.kind)
                    .is_some_and(|other| channel.options.intersect(&other.options))
            });
        }
        for channel in channels {
            result.push(ControlChannel {
                target: match channel.kind {
                    ChannelKind::Scalar => ControlTarget::Scalar(selection.clone()),
                    ChannelKind::Indexed => ControlTarget::Indexed(selection.clone()),
                    ChannelKind::Fixture(function) => ControlTarget::FixtureFunction {
                        selection: selection.clone(),
                        function,
                    },
                },
                label: format!("{} / {}", node.name, channel.name),
                cell_count: node.kind.cell_count(),
                options: channel.options,
            });
        }
    }
    Ok(result)
}
