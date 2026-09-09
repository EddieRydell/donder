use std::collections::{HashMap, HashSet, VecDeque};

use indexmap::IndexMap;

use crate::controller::{ControllerId, ControllerPortId};
use crate::element::{ColorCapability, ElementSelection};
use crate::fixture_profile::{DimmingCurve, FixtureProfileId};
use crate::fixture_profile::{FixtureChannelRole, FixtureProfileStore};
use crate::identity::SourceIdentity;
use dawn_runtime::fixture::{FixtureEncodingError, FixtureProgram};
pub use dawn_runtime::fixture::{apply_dimming_curve, quantize8, quantize16};
pub use dawn_runtime::patch::{ByteOrder, PatchValue, PatchValueLayout};
use dawn_runtime::patch::{ColorEncoding, FilterError, PreparedFilter};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PatchId(pub SourceIdentity);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct PatchNodeId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct PatchPortId(pub u16);

#[derive(Clone, Debug, PartialEq)]
pub struct PatchGraph {
    pub id: PatchId,
    pub nodes: IndexMap<PatchNodeId, PatchNode>,
    pub edges: Vec<PatchEdge>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatchNode {
    Source(PatchSource),
    Filter(FilterDefinition),
    Sink(PatchSink),
}

impl PatchNode {
    pub fn fixture_profile(&self) -> Option<&FixtureProfileId> {
        match self {
            Self::Source(PatchSource {
                output: PatchValueType::FixtureState { profile, .. },
                ..
            })
            | Self::Filter(FilterDefinition::FixtureProfileEncoding { profile, .. }) => {
                Some(profile)
            }
            _ => None,
        }
    }

    pub fn fixture_profile_mut(&mut self) -> Option<&mut FixtureProfileId> {
        match self {
            Self::Source(PatchSource {
                output: PatchValueType::FixtureState { profile, .. },
                ..
            })
            | Self::Filter(FilterDefinition::FixtureProfileEncoding { profile, .. }) => {
                Some(profile)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatchSource {
    pub selection: ElementSelection,
    pub output: PatchValueType,
}

impl PatchSource {
    pub fn validate_selection(
        &self,
        tree: &crate::element::ElementTree,
    ) -> Result<Vec<crate::element::ElementCellAddress>, String> {
        use crate::element::ElementNodeKind;
        let addresses = tree
            .flatten_selection(&self.selection)
            .map_err(|error| format!("Invalid patch source selection: {error:?}"))?;
        if addresses.len() != self.output.width() {
            return Err("Patch source width does not match its selected element span.".into());
        }
        if matches!(
            self.output,
            PatchValueType::Components { .. } | PatchValueType::Slots { .. }
        ) {
            return Err("Patch sources must use element values; components and slots are produced by filters.".into());
        }
        for address in &addresses {
            let kind = tree.nodes.get(&address.node).map(|node| &node.kind);
            let matches = match (&self.output, kind) {
                (PatchValueType::Color { .. }, Some(ElementNodeKind::Color { .. }))
                | (PatchValueType::Scalar { .. }, Some(ElementNodeKind::Scalar { .. }))
                | (PatchValueType::Indexed { .. }, Some(ElementNodeKind::Indexed { .. })) => true,
                (
                    PatchValueType::FixtureState { profile, .. },
                    Some(ElementNodeKind::Fixture { profile: selected }),
                ) => profile == selected,
                _ => false,
            };
            if !matches {
                return Err(format!(
                    "Patch source value type does not match selected element {}.",
                    address.node.0
                ));
            }
        }
        Ok(addresses)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchSink {
    pub controller: ControllerId,
    pub port: ControllerPortId,
    pub start_slot: u16,
    pub slot_count: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatchEdge {
    pub from: PatchNodeId,
    pub from_port: PatchPortId,
    pub to: PatchNodeId,
    pub to_port: PatchPortId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatchValueType {
    Color {
        width: usize,
    },
    Scalar {
        width: usize,
    },
    Indexed {
        width: usize,
    },
    FixtureState {
        width: usize,
        profile: FixtureProfileId,
    },
    Components {
        width: usize,
    },
    Slots {
        width: usize,
    },
}

impl PatchValueType {
    pub fn width(&self) -> usize {
        match self {
            Self::Color { width }
            | Self::Scalar { width }
            | Self::Indexed { width }
            | Self::FixtureState { width, .. }
            | Self::Components { width }
            | Self::Slots { width } => *width,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FilterDefinition {
    ScalarToComponents {
        width: usize,
    },
    ColorBreakdown {
        capability: ColorCapability,
        cell_count: usize,
    },
    DimmingCurve {
        curve: DimmingCurve,
        width: usize,
    },
    ScaleInvert {
        scale: f32,
        invert: bool,
        width: usize,
    },
    FanOut {
        width: usize,
        outputs: u16,
    },
    ComponentReorder {
        components_per_cell: u16,
        order: Vec<u16>,
        cell_count: usize,
    },
    IndexedValueMapping {
        entries: IndexMap<u32, f32>,
        width: usize,
    },
    Quantize8 {
        width: usize,
    },
    Quantize16 {
        width: usize,
        byte_order: ByteOrder,
    },
    FixtureProfileEncoding {
        profile: FixtureProfileId,
        fixture_count: usize,
        slot_count: usize,
    },
}

impl PatchValueType {
    pub fn layout(
        &self,
        profiles: &FixtureProfileStore,
    ) -> Result<PatchValueLayout, FilterEvaluationError> {
        let width =
            u32::try_from(self.width()).map_err(|_| FilterEvaluationError::ProgramTooLarge)?;
        Ok(match self {
            Self::Color { .. } => PatchValueLayout::Color(width),
            Self::Scalar { .. } => PatchValueLayout::Scalar(width),
            Self::Indexed { .. } => PatchValueLayout::Indexed(width),
            Self::Components { .. } => PatchValueLayout::Components(width),
            Self::Slots { .. } => PatchValueLayout::Slots(width),
            Self::FixtureState { profile, .. } => {
                let functions = profiles
                    .definitions
                    .get(profile)
                    .ok_or_else(|| FilterEvaluationError::MissingFixtureProfile(profile.clone()))?
                    .functions
                    .len();
                PatchValueLayout::Fixture {
                    width,
                    functions: u32::try_from(functions)
                        .map_err(|_| FilterEvaluationError::ProgramTooLarge)?,
                }
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FilterEvaluationError {
    TypeMismatch,
    WidthMismatch { expected: usize, actual: usize },
    UnsupportedDiscreteColor(crate::values::Color),
    MissingIndexedMapping(u32),
    MissingFixtureProfile(FixtureProfileId),
    MissingFixtureFunction,
    MissingFixtureEntry,
    InvalidFixtureSlot,
    InvalidFixtureBuffer,
    ProgramTooLarge,
}

pub fn evaluate_filter(
    filter: &FilterDefinition,
    input: &PatchValue,
    profiles: &FixtureProfileStore,
) -> Result<Vec<PatchValue>, FilterEvaluationError> {
    let mut outputs = (0..filter.output_port_count())
        .map(|port| {
            let output_type = filter
                .output_type(PatchPortId(port))
                .ok_or(FilterEvaluationError::TypeMismatch)?;
            Ok::<_, FilterEvaluationError>(PatchValue::new(output_type.layout(profiles)?))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let FilterDefinition::FixtureProfileEncoding {
        profile,
        fixture_count,
        slot_count,
    } = filter
    {
        let PatchValue::FixtureStates(states) = input else {
            return Err(FilterEvaluationError::TypeMismatch);
        };
        check_width(*fixture_count, states.len())?;
        let profile = profiles
            .definitions
            .get(profile)
            .ok_or_else(|| FilterEvaluationError::MissingFixtureProfile(profile.clone()))?;
        let program = prepare_fixture_encoding(profile, *slot_count)?;
        let [PatchValue::Slots(output)] = outputs.as_mut_slice() else {
            return Err(FilterEvaluationError::TypeMismatch);
        };
        output.resize(
            states
                .len()
                .checked_mul(*slot_count)
                .ok_or(FilterEvaluationError::ProgramTooLarge)?,
            0,
        );
        program.encode(states, output)?;
    } else {
        prepare_filter(filter)?.evaluate(input, &mut outputs)?;
    }
    Ok(outputs)
}

pub fn prepare_filter(filter: &FilterDefinition) -> Result<PreparedFilter, FilterEvaluationError> {
    let width = |value| u32::try_from(value).map_err(|_| FilterEvaluationError::ProgramTooLarge);
    Ok(match filter {
        FilterDefinition::ColorBreakdown {
            capability,
            cell_count,
        } => {
            let capability = match capability {
                ColorCapability::Rgb => ColorEncoding::Rgb,
                ColorCapability::Rgbw => ColorEncoding::Rgbw,
                ColorCapability::Discrete { emitters, mappings } => {
                    width(
                        emitters
                            .len()
                            .checked_mul(mappings.len())
                            .ok_or(FilterEvaluationError::ProgramTooLarge)?,
                    )?;
                    ColorEncoding::Discrete {
                        colors: mappings.iter().map(|mapping| mapping.color).collect(),
                        levels: mappings
                            .iter()
                            .flat_map(|mapping| {
                                emitters.iter().map(|emitter| {
                                    mapping.levels.get(&emitter.id).copied().unwrap_or(0.0)
                                })
                            })
                            .collect(),
                        components: width(emitters.len())?,
                    }
                }
            };
            PreparedFilter::ColorBreakdown {
                capability,
                cell_count: width(*cell_count)?,
            }
        }
        FilterDefinition::DimmingCurve {
            curve,
            width: count,
        } => PreparedFilter::DimmingCurve {
            curve: curve.clone(),
            width: width(*count)?,
        },
        FilterDefinition::ScaleInvert {
            scale,
            invert,
            width: count,
        } => PreparedFilter::ScaleInvert {
            scale: *scale,
            invert: *invert,
            width: width(*count)?,
        },
        FilterDefinition::FanOut {
            width: count,
            outputs,
        } => PreparedFilter::FanOut {
            width: width(*count)?,
            outputs: *outputs,
        },
        FilterDefinition::ComponentReorder {
            components_per_cell,
            order,
            cell_count,
        } => PreparedFilter::ComponentReorder {
            components_per_cell: *components_per_cell,
            order: order.clone().into_boxed_slice(),
            cell_count: width(*cell_count)?,
        },
        FilterDefinition::IndexedValueMapping {
            entries,
            width: count,
        } => {
            let mut entries = entries
                .iter()
                .map(|(id, value)| (*id, *value))
                .collect::<Vec<_>>();
            entries.sort_by_key(|(id, _)| *id);
            PreparedFilter::IndexedValueMapping {
                entries: entries.into_boxed_slice(),
                width: width(*count)?,
            }
        }
        FilterDefinition::ScalarToComponents { width: count } => {
            PreparedFilter::ScalarToComponents {
                width: width(*count)?,
            }
        }
        FilterDefinition::Quantize8 { width: count } => PreparedFilter::Quantize8 {
            width: width(*count)?,
        },
        FilterDefinition::Quantize16 {
            width: count,
            byte_order,
        } => PreparedFilter::Quantize16 {
            width: width(*count)?,
            byte_order: *byte_order,
        },
        FilterDefinition::FixtureProfileEncoding { .. } => {
            return Err(FilterEvaluationError::TypeMismatch);
        }
    })
}

impl From<FilterError> for FilterEvaluationError {
    fn from(error: FilterError) -> Self {
        match error {
            FilterError::TypeMismatch => Self::TypeMismatch,
            FilterError::WidthMismatch { expected, actual } => {
                Self::WidthMismatch { expected, actual }
            }
            FilterError::UnsupportedDiscreteColor(color) => Self::UnsupportedDiscreteColor(color),
            FilterError::MissingIndexedMapping(value) => Self::MissingIndexedMapping(value),
        }
    }
}

fn check_width(expected: usize, actual: usize) -> Result<(), FilterEvaluationError> {
    if expected == actual {
        Ok(())
    } else {
        Err(FilterEvaluationError::WidthMismatch { expected, actual })
    }
}

pub fn prepare_fixture_encoding(
    profile: &crate::fixture_profile::FixtureProfile,
    slot_count: usize,
) -> Result<FixtureProgram, FilterEvaluationError> {
    use crate::fixture_profile::FixtureFunctionKind;
    use dawn_runtime::fixture::{FixtureChannel, FixtureEncoding, FixtureEntry, FixtureFunction};
    let functions = profile.functions.iter().map(|(id, definition)| {
        let entries = match &definition.kind {
            FixtureFunctionKind::Indexed { entries } | FixtureFunctionKind::ColorWheel { entries } => {
                let mut entries = entries.iter().map(|entry| FixtureEntry {
                    id: entry.id, min: entry.dmx_min, max: entry.dmx_max,
                }).collect::<Vec<_>>();
                entries.sort_by_key(|entry| entry.id);
                Some(entries.into_boxed_slice())
            }
            _ => None,
        };
        FixtureFunction {
            id: *id,
            curve: definition.curve.clone(),
            entries,
            has_fine: profile.channels.iter().any(|channel| matches!(channel.role, FixtureChannelRole::Fine { function } if function == *id)),
        }
    }).collect();
    let channels = profile
        .channels
        .iter()
        .map(|channel| {
            if usize::from(channel.slot) >= slot_count {
                return Err(FilterEvaluationError::InvalidFixtureSlot);
            }
            let encoding = match channel.role {
                FixtureChannelRole::Ignored => FixtureEncoding::Ignored,
                FixtureChannelRole::ColorComponent {
                    function,
                    component,
                } => {
                    let definition = profile
                        .functions
                        .get(&function)
                        .ok_or(FilterEvaluationError::MissingFixtureFunction)?;
                    let FixtureFunctionKind::ColorMixing { model } = definition.kind else {
                        return Err(FilterEvaluationError::TypeMismatch);
                    };
                    FixtureEncoding::Color {
                        function,
                        component,
                        subtract_white: model == crate::fixture_profile::ColorMixingModel::Rgbw,
                    }
                }
                FixtureChannelRole::Coarse { function } | FixtureChannelRole::Fine { function } => {
                    let function = profile
                        .functions
                        .get_index_of(&function)
                        .ok_or(FilterEvaluationError::MissingFixtureFunction)?;
                    FixtureEncoding::Value {
                        function: u32::try_from(function)
                            .map_err(|_| FilterEvaluationError::ProgramTooLarge)?,
                        fine: matches!(channel.role, FixtureChannelRole::Fine { .. }),
                    }
                }
            };
            Ok(FixtureChannel {
                slot: channel.slot,
                encoding,
                curve: channel.curve.clone(),
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(FixtureProgram {
        functions,
        channels,
        slot_count: u32::try_from(slot_count)
            .map_err(|_| FilterEvaluationError::ProgramTooLarge)?,
    })
}

impl From<FixtureEncodingError> for FilterEvaluationError {
    fn from(error: FixtureEncodingError) -> Self {
        match error {
            FixtureEncodingError::TypeMismatch => Self::TypeMismatch,
            FixtureEncodingError::MissingFunction => Self::MissingFixtureFunction,
            FixtureEncodingError::MissingEntry => Self::MissingFixtureEntry,
            FixtureEncodingError::InvalidSlot => Self::InvalidFixtureSlot,
            FixtureEncodingError::WidthMismatch => Self::InvalidFixtureBuffer,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatchValidationError {
    MissingNode(PatchNodeId),
    InvalidPort {
        node: PatchNodeId,
        port: PatchPortId,
    },
    SourceHasInput(PatchNodeId),
    SinkHasOutput(PatchNodeId),
    MissingInput {
        node: PatchNodeId,
        port: PatchPortId,
    },
    MultipleInputs {
        node: PatchNodeId,
        port: PatchPortId,
    },
    Cycle,
    TypeMismatch {
        from: PatchNodeId,
        to: PatchNodeId,
    },
    InvalidWidth(PatchNodeId),
    SinkWidthMismatch {
        node: PatchNodeId,
        value_width: usize,
        slot_count: u16,
    },
    DestinationOverlap {
        controller: ControllerId,
        port: ControllerPortId,
    },
    InvalidFilter(PatchNodeId),
}

impl std::fmt::Display for PatchValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingNode(node) => write!(
                f,
                "Connection references missing node {}. Choose an existing node or remove the connection.",
                node.0
            ),
            Self::InvalidPort { node, port } => write!(
                f,
                "Node {} has no port {}. Input ports start at zero; only fan-out filters have multiple output ports.",
                node.0, port.0
            ),
            Self::SourceHasInput(node) => {
                write!(f, "Source node {} cannot receive a connection.", node.0)
            }
            Self::SinkHasOutput(node) => write!(
                f,
                "Controller output node {} cannot send a connection.",
                node.0
            ),
            Self::MissingInput { node, port } => write!(
                f,
                "Connect input {} of node {} before applying the patch.",
                port.0, node.0
            ),
            Self::MultipleInputs { node, port } => write!(
                f,
                "Input {} of node {} has multiple connections. Keep exactly one.",
                port.0, node.0
            ),
            Self::Cycle => write!(
                f,
                "Connections form a cycle. Remove the connection that feeds back into an earlier node."
            ),
            Self::TypeMismatch { from, to } => write!(
                f,
                "Node {} output does not match node {} input. Match both value type and value count, adding conversion filters where needed.",
                from.0, to.0
            ),
            Self::InvalidWidth(node) => write!(
                f,
                "Node {} needs a positive value or channel count.",
                node.0
            ),
            Self::SinkWidthMismatch {
                node,
                value_width,
                slot_count,
            } => write!(
                f,
                "Output node {} receives {} values but reserves {} channels. Make the counts match.",
                node.0, value_width, slot_count
            ),
            Self::DestinationOverlap { controller, port } => write!(
                f,
                "Controller {} port {} has overlapping channel assignments. Choose separate channel ranges.",
                controller.0.object(),
                port.0
            ),
            Self::InvalidFilter(node) => write!(
                f,
                "Filter node {} is invalid. Check its counts, curve values, component order, and mappings.",
                node.0
            ),
        }
    }
}

impl FilterDefinition {
    pub fn input_type(&self) -> PatchValueType {
        match self {
            Self::ScalarToComponents { width } => PatchValueType::Scalar { width: *width },
            Self::ColorBreakdown { cell_count, .. } => PatchValueType::Color { width: *cell_count },
            Self::DimmingCurve { width, .. } | Self::ScaleInvert { width, .. } => {
                PatchValueType::Components { width: *width }
            }
            Self::FanOut { width, .. } => PatchValueType::Components { width: *width },
            Self::ComponentReorder {
                components_per_cell,
                cell_count,
                ..
            } => PatchValueType::Components {
                width: usize::from(*components_per_cell) * *cell_count,
            },
            Self::IndexedValueMapping { width, .. } => PatchValueType::Indexed { width: *width },
            Self::Quantize8 { width } | Self::Quantize16 { width, .. } => {
                PatchValueType::Components { width: *width }
            }
            Self::FixtureProfileEncoding {
                profile,
                fixture_count,
                ..
            } => PatchValueType::FixtureState {
                width: *fixture_count,
                profile: profile.clone(),
            },
        }
    }

    pub fn output_type(&self, port: PatchPortId) -> Option<PatchValueType> {
        match self {
            Self::ColorBreakdown {
                capability,
                cell_count,
            } => Some(PatchValueType::Components {
                width: color_component_count(capability) * *cell_count,
            }),
            Self::DimmingCurve { width, .. }
            | Self::ScaleInvert { width, .. }
            | Self::IndexedValueMapping { width, .. }
            | Self::ScalarToComponents { width } => {
                Some(PatchValueType::Components { width: *width })
            }
            Self::FanOut { width, outputs } if port.0 < *outputs => {
                Some(PatchValueType::Components { width: *width })
            }
            Self::FanOut { .. } => None,
            Self::ComponentReorder {
                components_per_cell,
                cell_count,
                ..
            } => Some(PatchValueType::Components {
                width: usize::from(*components_per_cell) * *cell_count,
            }),
            Self::Quantize8 { width } => Some(PatchValueType::Slots { width: *width }),
            Self::Quantize16 { width, .. } => Some(PatchValueType::Slots { width: *width * 2 }),
            Self::FixtureProfileEncoding {
                fixture_count,
                slot_count,
                ..
            } => Some(PatchValueType::Slots {
                width: *fixture_count * *slot_count,
            }),
        }
    }

    pub fn output_port_count(&self) -> u16 {
        match self {
            Self::FanOut { outputs, .. } => *outputs,
            _ => 1,
        }
    }

    pub fn validate(&self) -> bool {
        match self {
            Self::ColorBreakdown {
                capability,
                cell_count,
            } => *cell_count > 0 && capability.validate().is_ok(),
            Self::DimmingCurve { curve, width } => {
                *width > 0 && crate::fixture_profile::validate_curve(curve).is_ok()
            }
            Self::ScaleInvert { scale, width, .. } => scale.is_finite() && *width > 0,
            Self::FanOut { width, outputs } => *width > 0 && *outputs > 0,
            Self::ComponentReorder {
                components_per_cell,
                order,
                cell_count,
            } => {
                *components_per_cell > 0
                    && *cell_count > 0
                    && order.len() == usize::from(*components_per_cell)
                    && order.iter().copied().collect::<HashSet<_>>().len() == order.len()
                    && order
                        .iter()
                        .all(|component| *component < *components_per_cell)
            }
            Self::IndexedValueMapping { entries, width } => {
                *width > 0
                    && !entries.is_empty()
                    && entries
                        .values()
                        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
            }
            Self::Quantize8 { width }
            | Self::Quantize16 { width, .. }
            | Self::ScalarToComponents { width } => *width > 0,
            Self::FixtureProfileEncoding {
                fixture_count,
                slot_count,
                ..
            } => *fixture_count > 0 && *slot_count > 0,
        }
    }
}

impl PatchGraph {
    /// Remove one destination and prune only upstream nodes no longer used by
    /// another destination. Shared routing branches retain their identity.
    pub fn remove_output(&mut self, sink: PatchNodeId) -> Result<(), String> {
        if !matches!(self.nodes.get(&sink), Some(PatchNode::Sink(_))) {
            return Err("Choose an output destination to remove.".to_string());
        }
        let mut pending = vec![sink];
        while let Some(node) = pending.pop() {
            if node != sink && self.edges.iter().any(|edge| edge.from == node) {
                continue;
            }
            let inputs = self
                .edges
                .iter()
                .filter(|edge| edge.to == node)
                .map(|edge| edge.from)
                .collect::<Vec<_>>();
            self.edges
                .retain(|edge| edge.from != node && edge.to != node);
            self.nodes.shift_remove(&node);
            pending.extend(inputs);
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<Vec<PatchNodeId>, PatchValidationError> {
        for (id, node) in &self.nodes {
            if node_width(node) == 0 {
                return Err(PatchValidationError::InvalidWidth(*id));
            }
            if let PatchNode::Filter(filter) = node
                && !filter.validate()
            {
                return Err(PatchValidationError::InvalidFilter(*id));
            }
        }
        let mut incoming: HashMap<(PatchNodeId, PatchPortId), usize> = HashMap::new();
        let mut outgoing: HashMap<PatchNodeId, Vec<PatchNodeId>> = HashMap::new();
        let mut indegree: HashMap<PatchNodeId, usize> =
            self.nodes.keys().map(|id| (*id, 0)).collect();
        for edge in &self.edges {
            let from = self
                .nodes
                .get(&edge.from)
                .ok_or(PatchValidationError::MissingNode(edge.from))?;
            let to = self
                .nodes
                .get(&edge.to)
                .ok_or(PatchValidationError::MissingNode(edge.to))?;
            if matches!(from, PatchNode::Sink(_)) {
                return Err(PatchValidationError::SinkHasOutput(edge.from));
            }
            if matches!(to, PatchNode::Source(_)) {
                return Err(PatchValidationError::SourceHasInput(edge.to));
            }
            let output =
                output_type(from, edge.from_port).ok_or(PatchValidationError::InvalidPort {
                    node: edge.from,
                    port: edge.from_port,
                })?;
            if edge.to_port != PatchPortId(0) {
                return Err(PatchValidationError::InvalidPort {
                    node: edge.to,
                    port: edge.to_port,
                });
            }
            let input = input_type(to).ok_or(PatchValidationError::InvalidPort {
                node: edge.to,
                port: edge.to_port,
            })?;
            if output != input {
                return Err(PatchValidationError::TypeMismatch {
                    from: edge.from,
                    to: edge.to,
                });
            }
            let count = incoming.entry((edge.to, edge.to_port)).or_default();
            *count += 1;
            if *count > 1 {
                return Err(PatchValidationError::MultipleInputs {
                    node: edge.to,
                    port: edge.to_port,
                });
            }
            outgoing.entry(edge.from).or_default().push(edge.to);
            *indegree.entry(edge.to).or_default() += 1;
        }
        for (id, node) in &self.nodes {
            if !matches!(node, PatchNode::Source(_))
                && incoming.get(&(*id, PatchPortId(0))).copied() != Some(1)
            {
                return Err(PatchValidationError::MissingInput {
                    node: *id,
                    port: PatchPortId(0),
                });
            }
            if let PatchNode::Sink(sink) = node {
                let input = input_type(node).unwrap_or(PatchValueType::Slots { width: 0 });
                if input.width() != usize::from(sink.slot_count) {
                    return Err(PatchValidationError::SinkWidthMismatch {
                        node: *id,
                        value_width: input.width(),
                        slot_count: sink.slot_count,
                    });
                }
            }
        }
        validate_destinations(&self.nodes)?;
        let mut ready = self
            .nodes
            .keys()
            .filter(|id| indegree[id] == 0)
            .copied()
            .collect::<VecDeque<_>>();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(id) = ready.pop_front() {
            order.push(id);
            if let Some(next) = outgoing.get(&id) {
                for target in next {
                    let degree = indegree
                        .get_mut(target)
                        .ok_or(PatchValidationError::MissingNode(*target))?;
                    *degree -= 1;
                    if *degree == 0 {
                        // Finish ready consumers before starting another independent branch.
                        ready.push_front(*target);
                    }
                }
            }
        }
        if order.len() != self.nodes.len() {
            return Err(PatchValidationError::Cycle);
        }
        Ok(order)
    }
}

fn node_width(node: &PatchNode) -> usize {
    match node {
        PatchNode::Source(source) => source.output.width(),
        PatchNode::Filter(filter) => filter
            .output_type(PatchPortId(0))
            .map_or(0, |value| value.width()),
        PatchNode::Sink(sink) => usize::from(sink.slot_count),
    }
}

fn input_type(node: &PatchNode) -> Option<PatchValueType> {
    match node {
        PatchNode::Source(_) => None,
        PatchNode::Filter(filter) => Some(filter.input_type()),
        PatchNode::Sink(sink) => Some(PatchValueType::Slots {
            width: usize::from(sink.slot_count),
        }),
    }
}

fn output_type(node: &PatchNode, port: PatchPortId) -> Option<PatchValueType> {
    match node {
        PatchNode::Source(source) if port == PatchPortId(0) => Some(source.output.clone()),
        PatchNode::Source(_) => None,
        PatchNode::Filter(filter) => filter.output_type(port),
        PatchNode::Sink(_) => None,
    }
}

fn validate_destinations(
    nodes: &IndexMap<PatchNodeId, PatchNode>,
) -> Result<(), PatchValidationError> {
    let mut destinations: HashMap<(ControllerId, ControllerPortId), Vec<(u16, u16)>> =
        HashMap::new();
    for node in nodes.values() {
        let PatchNode::Sink(sink) = node else {
            continue;
        };
        let end = sink
            .start_slot
            .checked_add(sink.slot_count)
            .ok_or_else(|| PatchValidationError::DestinationOverlap {
                controller: sink.controller.clone(),
                port: sink.port,
            })?;
        let ranges = destinations
            .entry((sink.controller.clone(), sink.port))
            .or_default();
        if ranges
            .iter()
            .any(|(start, existing_end)| sink.start_slot < *existing_end && *start < end)
        {
            return Err(PatchValidationError::DestinationOverlap {
                controller: sink.controller.clone(),
                port: sink.port,
            });
        }
        ranges.push((sink.start_slot, end));
    }
    Ok(())
}

pub fn color_component_count(capability: &ColorCapability) -> usize {
    match capability {
        ColorCapability::Rgb => 3,
        ColorCapability::Rgbw => 4,
        ColorCapability::Discrete { emitters, .. } => emitters.len(),
    }
}
