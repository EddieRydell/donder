use dawn_language::controller::*;
use dawn_language::element::*;
use dawn_language::fixture_profile::*;
use dawn_language::identity::DocumentId;
use dawn_language::patch::*;
use dawn_language::preview::*;
use dawn_language::setup::Setup;
use yaml_serde::{Mapping, Value};

use super::ProjectSession;
use super::values::*;
use crate::ExportProjectError;
use crate::source::SourceObjectKind;

pub(super) fn setup_value(
    session: &ProjectSession,
    from: &DocumentId,
    setup: &Setup,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("setup");
    value.insert(
        string_value("elements"),
        Value::String(write_source_reference(
            session,
            from,
            SourceObjectKind::ElementTree,
            &setup.elements.0,
        )?),
    );
    value.insert(
        string_value("preview"),
        Value::String(write_source_reference(
            session,
            from,
            SourceObjectKind::PreviewLayout,
            &setup.preview.0,
        )?),
    );
    value.insert(
        string_value("patch"),
        Value::String(write_source_reference(
            session,
            from,
            SourceObjectKind::Patch,
            &setup.patch.0,
        )?),
    );
    value.insert(
        string_value("controllers"),
        Value::Sequence(
            setup
                .controllers
                .iter()
                .map(|id| {
                    write_source_reference(session, from, SourceObjectKind::Controller, &id.0)
                        .map(Value::String)
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn controller_value(controller: &Controller) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("controller");
    let mut protocol = Mapping::new();
    match &controller.protocol {
        ControllerProtocol::E131(config) => {
            protocol.insert(string_value("type"), Value::String("e131".to_string()));
            protocol.insert(
                string_value("source_name"),
                Value::String(config.source_name.clone()),
            );
            protocol.insert(
                string_value("bind_address"),
                Value::String(config.bind_address.to_string()),
            );
            protocol.insert(string_value("priority"), serialized_value(config.priority)?);
            match config.mode {
                E131Mode::Multicast => {
                    protocol.insert(string_value("mode"), Value::String("multicast".to_string()));
                }
                E131Mode::Unicast { destination } => {
                    protocol.insert(string_value("mode"), Value::String("unicast".to_string()));
                    protocol.insert(
                        string_value("destination"),
                        Value::String(destination.to_string()),
                    );
                }
            }
        }
        ControllerProtocol::ArtNet(config) => {
            protocol.insert(string_value("type"), Value::String("artnet".to_string()));
            protocol.insert(
                string_value("bind_address"),
                Value::String(config.bind_address.to_string()),
            );
            protocol.insert(
                string_value("destination"),
                Value::String(config.destination.to_string()),
            );
            protocol.insert(
                string_value("mode"),
                Value::String(
                    match config.mode {
                        ArtNetMode::Unicast => "unicast",
                        ArtNetMode::Broadcast => "broadcast",
                    }
                    .to_string(),
                ),
            );
        }
    }
    value.insert(string_value("protocol"), Value::Mapping(protocol));
    value.insert(
        string_value("ports"),
        Value::Sequence(
            controller
                .ports
                .iter()
                .map(|port| {
                    let mut item = Mapping::new();
                    item.insert(string_value("id"), serialized_value(port.id.0)?);
                    match port.address {
                        ControllerPortAddress::E131Universe(universe) => {
                            item.insert(string_value("universe"), serialized_value(universe)?)
                        }
                        ControllerPortAddress::ArtNetPort(address) => {
                            item.insert(string_value("port_address"), serialized_value(address)?)
                        }
                    };
                    item.insert(
                        string_value("slot_count"),
                        serialized_value(port.slot_count)?,
                    );
                    Ok(Value::Mapping(item))
                })
                .collect::<Result<Vec<_>, ExportProjectError>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn element_tree_value(
    session: &ProjectSession,
    from: &DocumentId,
    tree: &ElementTree,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("element_tree");
    value.insert(
        string_value("roots"),
        Value::Sequence(
            tree.roots
                .iter()
                .map(|id| serialized_value(id.0))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("nodes"),
        Value::Sequence(
            tree.nodes
                .iter()
                .map(|(id, node)| {
                    let mut item = Mapping::new();
                    item.insert(string_value("id"), serialized_value(id.0)?);
                    item.insert(string_value("name"), Value::String(node.name.clone()));
                    match &node.kind {
                        ElementNodeKind::Group { children } => {
                            item.insert(string_value("type"), Value::String("group".to_string()));
                            item.insert(
                                string_value("children"),
                                Value::Sequence(
                                    children
                                        .iter()
                                        .map(|child| serialized_value(child.0))
                                        .collect::<Result<Vec<_>, _>>()?,
                                ),
                            );
                        }
                        ElementNodeKind::Color { cells, capability } => {
                            item.insert(string_value("type"), Value::String("color".to_string()));
                            item.insert(string_value("cells"), serialized_value(*cells)?);
                            item.insert(
                                string_value("capability"),
                                color_capability_value(capability)?,
                            );
                        }
                        ElementNodeKind::Scalar { cells } => {
                            item.insert(string_value("type"), Value::String("scalar".to_string()));
                            item.insert(string_value("cells"), serialized_value(*cells)?);
                        }
                        ElementNodeKind::Indexed { cells, options } => {
                            item.insert(string_value("type"), Value::String("indexed".to_string()));
                            item.insert(string_value("cells"), serialized_value(*cells)?);
                            item.insert(
                                string_value("options"),
                                Value::Sequence(
                                    options
                                        .iter()
                                        .map(|option| {
                                            let mut option_value = Mapping::new();
                                            option_value.insert(
                                                string_value("id"),
                                                serialized_value(option.id.0)?,
                                            );
                                            option_value.insert(
                                                string_value("name"),
                                                Value::String(option.name.clone()),
                                            );
                                            Ok(Value::Mapping(option_value))
                                        })
                                        .collect::<Result<Vec<_>, ExportProjectError>>()?,
                                ),
                            );
                        }
                        ElementNodeKind::Fixture { profile } => {
                            item.insert(string_value("type"), Value::String("fixture".to_string()));
                            item.insert(
                                string_value("profile"),
                                Value::String(write_source_reference(
                                    session,
                                    from,
                                    SourceObjectKind::FixtureProfile,
                                    &profile.0,
                                )?),
                            );
                        }
                    }
                    Ok(Value::Mapping(item))
                })
                .collect::<Result<Vec<_>, ExportProjectError>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn preview_layout_value(
    session: &ProjectSession,
    from: &DocumentId,
    layout: &PreviewLayout,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("preview_layout");
    value.insert(
        string_value("element_tree"),
        Value::String(write_source_reference(
            session,
            from,
            SourceObjectKind::ElementTree,
            &layout.element_tree.0,
        )?),
    );
    value.insert(
        string_value("props"),
        Value::Sequence(
            layout
                .props
                .iter()
                .map(|prop| {
                    let mut item = Mapping::new();
                    item.insert(string_value("id"), serialized_value(prop.id.0)?);
                    item.insert(string_value("name"), Value::String(prop.name.clone()));
                    item.insert(
                        string_value("prop"),
                        Value::String(write_source_reference(
                            session,
                            from,
                            SourceObjectKind::PropDefinition,
                            &prop.definition.0,
                        )?),
                    );
                    item.insert(string_value("transform"), transform_value(prop)?);
                    item.insert(
                        string_value("bindings"),
                        Value::Sequence(
                            prop.bindings
                                .iter()
                                .map(|binding| {
                                    let mut binding_value = Mapping::new();
                                    binding_value.insert(
                                        string_value("node"),
                                        serialized_value(binding.node.0)?,
                                    );
                                    binding_value.insert(
                                        string_value("cell"),
                                        serialized_value(binding.cell)?,
                                    );
                                    Ok(Value::Mapping(binding_value))
                                })
                                .collect::<Result<Vec<_>, ExportProjectError>>()?,
                        ),
                    );
                    Ok(Value::Mapping(item))
                })
                .collect::<Result<Vec<_>, ExportProjectError>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn prop_definition_value(
    definition: &PropDefinition,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("prop");
    value.insert(
        string_value("bulb_diameter"),
        serialized_value(definition.bulb_radius.as_meters_f32() * 2.0)?,
    );
    value.insert(
        string_value("geometry"),
        geometry_value(&definition.geometry)?,
    );
    Ok(Value::Mapping(value))
}

pub(super) fn patch_value(
    session: &ProjectSession,
    from: &DocumentId,
    patch: &PatchGraph,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("patch");
    value.insert(
        string_value("nodes"),
        Value::Sequence(
            patch
                .nodes
                .iter()
                .map(|(id, node)| patch_node_value(session, from, *id, node))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("edges"),
        Value::Sequence(
            patch
                .edges
                .iter()
                .map(|edge| {
                    let mut item = Mapping::new();
                    item.insert(string_value("from"), serialized_value(edge.from.0)?);
                    item.insert(
                        string_value("from_port"),
                        serialized_value(edge.from_port.0)?,
                    );
                    item.insert(string_value("to"), serialized_value(edge.to.0)?);
                    item.insert(string_value("to_port"), serialized_value(edge.to_port.0)?);
                    Ok(Value::Mapping(item))
                })
                .collect::<Result<Vec<_>, ExportProjectError>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

fn patch_node_value(
    session: &ProjectSession,
    from: &DocumentId,
    id: PatchNodeId,
    node: &PatchNode,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), serialized_value(id.0)?);
    match node {
        PatchNode::Source(source) => {
            value.insert(string_value("type"), Value::String("source".to_string()));
            value.insert(
                string_value("selection"),
                element_selection_value(session, from, &source.selection)?,
            );
            let (kind, width) = match &source.output {
                PatchValueType::Color { width } => ("color", *width),
                PatchValueType::Scalar { width } => ("scalar", *width),
                PatchValueType::Indexed { width } => ("indexed", *width),
                PatchValueType::FixtureState { width, profile } => {
                    value.insert(
                        string_value("profile"),
                        Value::String(write_source_reference(
                            session,
                            from,
                            SourceObjectKind::FixtureProfile,
                            &profile.0,
                        )?),
                    );
                    ("fixture_state", *width)
                }
                _ => {
                    return Err(ExportProjectError::InvalidReference {
                        path: from.path().to_path_buf(),
                        reference: id.0.to_string(),
                        message: "patch source has a non-source value type".to_string(),
                    });
                }
            };
            value.insert(string_value("output"), Value::String(kind.to_string()));
            value.insert(string_value("width"), serialized_value(width)?);
        }
        PatchNode::Filter(filter) => {
            value.insert(string_value("type"), Value::String("filter".to_string()));
            write_filter(session, from, &mut value, filter)?;
        }
        PatchNode::Sink(sink) => {
            value.insert(string_value("type"), Value::String("sink".to_string()));
            value.insert(
                string_value("controller"),
                Value::String(write_source_reference(
                    session,
                    from,
                    SourceObjectKind::Controller,
                    &sink.controller.0,
                )?),
            );
            value.insert(string_value("port"), serialized_value(sink.port.0)?);
            value.insert(
                string_value("start_slot"),
                serialized_value(sink.start_slot)?,
            );
            value.insert(
                string_value("slot_count"),
                serialized_value(sink.slot_count)?,
            );
        }
    }
    Ok(Value::Mapping(value))
}

fn write_filter(
    session: &ProjectSession,
    from: &DocumentId,
    value: &mut Mapping,
    filter: &FilterDefinition,
) -> Result<(), ExportProjectError> {
    match filter {
        FilterDefinition::ColorBreakdown {
            capability,
            cell_count,
        } => {
            value.insert(
                string_value("filter"),
                Value::String("color_breakdown".to_string()),
            );
            value.insert(
                string_value("capability"),
                color_capability_value(capability)?,
            );
            value.insert(string_value("cell_count"), serialized_value(*cell_count)?);
        }
        FilterDefinition::DimmingCurve { curve, width } => {
            value.insert(
                string_value("filter"),
                Value::String("dimming_curve".to_string()),
            );
            value.insert(string_value("curve"), dimming_curve_value(curve)?);
            value.insert(string_value("width"), serialized_value(*width)?);
        }
        FilterDefinition::ScaleInvert {
            scale,
            invert,
            width,
        } => {
            value.insert(
                string_value("filter"),
                Value::String("scale_invert".to_string()),
            );
            value.insert(string_value("scale"), serialized_value(*scale)?);
            value.insert(string_value("invert"), Value::Bool(*invert));
            value.insert(string_value("width"), serialized_value(*width)?);
        }
        FilterDefinition::FanOut { width, outputs } => {
            value.insert(string_value("filter"), Value::String("fan_out".to_string()));
            value.insert(string_value("width"), serialized_value(*width)?);
            value.insert(string_value("outputs"), serialized_value(*outputs)?);
        }
        FilterDefinition::ComponentReorder {
            components_per_cell,
            order,
            cell_count,
        } => {
            value.insert(
                string_value("filter"),
                Value::String("component_reorder".to_string()),
            );
            value.insert(
                string_value("components_per_cell"),
                serialized_value(*components_per_cell)?,
            );
            value.insert(
                string_value("order"),
                Value::Sequence(
                    order
                        .iter()
                        .map(|item| serialized_value(*item))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
            value.insert(string_value("cell_count"), serialized_value(*cell_count)?);
        }
        FilterDefinition::IndexedValueMapping { entries, width } => {
            value.insert(
                string_value("filter"),
                Value::String("indexed_value_mapping".to_string()),
            );
            value.insert(
                string_value("entries"),
                Value::Sequence(
                    entries
                        .iter()
                        .map(|(id, mapped)| {
                            let mut entry = Mapping::new();
                            entry.insert(string_value("id"), serialized_value(*id)?);
                            entry.insert(string_value("value"), serialized_value(*mapped)?);
                            Ok(Value::Mapping(entry))
                        })
                        .collect::<Result<Vec<_>, ExportProjectError>>()?,
                ),
            );
            value.insert(string_value("width"), serialized_value(*width)?);
        }
        FilterDefinition::ScalarToComponents { width } => {
            value.insert(
                string_value("filter"),
                Value::String("scalar_to_components".to_string()),
            );
            value.insert(string_value("width"), serialized_value(*width)?);
        }
        FilterDefinition::Quantize8 { width } => {
            value.insert(
                string_value("filter"),
                Value::String("quantize_8".to_string()),
            );
            value.insert(string_value("width"), serialized_value(*width)?);
        }
        FilterDefinition::Quantize16 { width, byte_order } => {
            value.insert(
                string_value("filter"),
                Value::String("quantize_16".to_string()),
            );
            value.insert(string_value("width"), serialized_value(*width)?);
            value.insert(
                string_value("byte_order"),
                Value::String(
                    match byte_order {
                        ByteOrder::CoarseFine => "coarse_fine",
                        ByteOrder::FineCoarse => "fine_coarse",
                    }
                    .to_string(),
                ),
            );
        }
        FilterDefinition::FixtureProfileEncoding {
            profile,
            fixture_count,
            slot_count,
        } => {
            value.insert(
                string_value("filter"),
                Value::String("fixture_profile_encoding".to_string()),
            );
            value.insert(
                string_value("profile"),
                Value::String(write_source_reference(
                    session,
                    from,
                    SourceObjectKind::FixtureProfile,
                    &profile.0,
                )?),
            );
            value.insert(
                string_value("fixture_count"),
                serialized_value(*fixture_count)?,
            );
            value.insert(string_value("slot_count"), serialized_value(*slot_count)?);
        }
    }
    Ok(())
}

pub(super) fn fixture_profile_value(profile: &FixtureProfile) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("fixture_profile");
    value.insert(
        string_value("functions"),
        Value::Sequence(
            profile
                .functions
                .iter()
                .map(|(id, function)| function_value(*id, function))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("channels"),
        Value::Sequence(
            profile
                .channels
                .iter()
                .map(channel_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("behavior_rules"),
        Value::Sequence(
            profile
                .behavior_rules
                .iter()
                .map(behavior_rule_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

fn function_value(
    id: FixtureFunctionId,
    function: &FixtureFunction,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), serialized_value(id.0)?);
    value.insert(string_value("name"), Value::String(function.name.clone()));
    if let Some(tag) = function.tag {
        value.insert(
            string_value("tag"),
            Value::String(function_tag_name(tag).to_string()),
        );
    }
    match &function.kind {
        FixtureFunctionKind::Range => {
            value.insert(string_value("type"), Value::String("range".to_string()));
        }
        FixtureFunctionKind::Indexed { entries } => {
            value.insert(string_value("type"), Value::String("indexed".to_string()));
            value.insert(string_value("entries"), entries_value(entries)?);
        }
        FixtureFunctionKind::ColorWheel { entries } => {
            value.insert(
                string_value("type"),
                Value::String("color_wheel".to_string()),
            );
            value.insert(string_value("entries"), entries_value(entries)?);
        }
        FixtureFunctionKind::ColorMixing { model } => {
            value.insert(
                string_value("type"),
                Value::String("color_mixing".to_string()),
            );
            value.insert(
                string_value("model"),
                Value::String(
                    match model {
                        ColorMixingModel::Rgb => "rgb",
                        ColorMixingModel::Rgbw => "rgbw",
                    }
                    .to_string(),
                ),
            );
        }
    }
    value.insert(string_value("curve"), dimming_curve_value(&function.curve)?);
    Ok(Value::Mapping(value))
}

fn entries_value(entries: &[FixtureIndexedEntry]) -> Result<Value, ExportProjectError> {
    Ok(Value::Sequence(
        entries
            .iter()
            .map(|entry| {
                let mut value = Mapping::new();
                value.insert(string_value("id"), serialized_value(entry.id.0)?);
                value.insert(string_value("name"), Value::String(entry.name.clone()));
                value.insert(string_value("dmx_min"), serialized_value(entry.dmx_min)?);
                value.insert(string_value("dmx_max"), serialized_value(entry.dmx_max)?);
                value.insert(
                    string_value("curve_control"),
                    Value::Bool(entry.curve_control),
                );
                if let Some(color) = entry.color {
                    value.insert(string_value("color"), Value::String(color.to_hex()));
                }
                if let Some(tag) = entry.tag {
                    value.insert(
                        string_value("tag"),
                        Value::String(entry_tag_name(tag).to_string()),
                    );
                }
                Ok(Value::Mapping(value))
            })
            .collect::<Result<Vec<_>, ExportProjectError>>()?,
    ))
}

fn channel_value(channel: &FixtureChannel) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("slot"), serialized_value(channel.slot)?);
    match channel.role {
        FixtureChannelRole::Coarse { function } => {
            value.insert(string_value("role"), Value::String("coarse".to_string()));
            value.insert(string_value("function"), serialized_value(function.0)?);
        }
        FixtureChannelRole::Fine { function } => {
            value.insert(string_value("role"), Value::String("fine".to_string()));
            value.insert(string_value("function"), serialized_value(function.0)?);
        }
        FixtureChannelRole::ColorComponent {
            function,
            component,
        } => {
            value.insert(
                string_value("role"),
                Value::String("color_component".to_string()),
            );
            value.insert(string_value("function"), serialized_value(function.0)?);
            value.insert(
                string_value("component"),
                Value::String(component_name(component).to_string()),
            );
        }
        FixtureChannelRole::Ignored => {
            value.insert(string_value("role"), Value::String("ignored".to_string()));
        }
    }
    value.insert(string_value("curve"), dimming_curve_value(&channel.curve)?);
    Ok(Value::Mapping(value))
}

fn behavior_rule_value(rule: &FixtureBehaviorRule) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match rule {
        FixtureBehaviorRule::Shutter {
            function,
            closed,
            open,
        } => {
            value.insert(string_value("type"), Value::String("shutter".to_string()));
            value.insert(string_value("function"), serialized_value(function.0)?);
            value.insert(string_value("closed"), serialized_value(closed.0)?);
            value.insert(string_value("open"), serialized_value(open.0)?);
        }
        FixtureBehaviorRule::Dimmer { function, off, on } => {
            value.insert(string_value("type"), Value::String("dimmer".to_string()));
            value.insert(string_value("function"), serialized_value(function.0)?);
            value.insert(string_value("off"), serialized_value(*off)?);
            value.insert(string_value("on"), serialized_value(*on)?);
        }
        FixtureBehaviorRule::ColorWheel { function, entries } => {
            value.insert(
                string_value("type"),
                Value::String("color_wheel".to_string()),
            );
            value.insert(string_value("function"), serialized_value(function.0)?);
            value.insert(
                string_value("entries"),
                Value::Sequence(
                    entries
                        .iter()
                        .map(|entry| {
                            let mut item = Mapping::new();
                            item.insert(string_value("color"), Value::String(entry.color.to_hex()));
                            item.insert(string_value("entry"), serialized_value(entry.entry.0)?);
                            Ok(Value::Mapping(item))
                        })
                        .collect::<Result<Vec<_>, ExportProjectError>>()?,
                ),
            );
        }
        FixtureBehaviorRule::PrismGate {
            function,
            disabled,
            enabled,
        } => {
            value.insert(
                string_value("type"),
                Value::String("prism_gate".to_string()),
            );
            value.insert(string_value("function"), serialized_value(function.0)?);
            value.insert(string_value("disabled"), serialized_value(disabled.0)?);
            value.insert(string_value("enabled"), serialized_value(enabled.0)?);
        }
    }
    Ok(Value::Mapping(value))
}

fn color_capability_value(capability: &ColorCapability) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match capability {
        ColorCapability::Rgb => {
            value.insert(string_value("type"), Value::String("rgb".to_string()));
        }
        ColorCapability::Rgbw => {
            value.insert(string_value("type"), Value::String("rgbw".to_string()));
        }
        ColorCapability::Discrete { emitters, mappings } => {
            value.insert(string_value("type"), Value::String("discrete".to_string()));
            value.insert(
                string_value("emitters"),
                Value::Sequence(
                    emitters
                        .iter()
                        .map(|emitter| {
                            let mut item = Mapping::new();
                            item.insert(string_value("id"), serialized_value(emitter.id.0)?);
                            item.insert(string_value("name"), Value::String(emitter.name.clone()));
                            Ok(Value::Mapping(item))
                        })
                        .collect::<Result<Vec<_>, ExportProjectError>>()?,
                ),
            );
            value.insert(
                string_value("mappings"),
                Value::Sequence(
                    mappings
                        .iter()
                        .map(|mapping| {
                            let mut item = Mapping::new();
                            item.insert(
                                string_value("color"),
                                Value::String(mapping.color.to_hex()),
                            );
                            item.insert(
                                string_value("levels"),
                                Value::Sequence(
                                    mapping
                                        .levels
                                        .iter()
                                        .map(|(emitter, level)| {
                                            let mut level_value = Mapping::new();
                                            level_value.insert(
                                                string_value("emitter"),
                                                serialized_value(emitter.0)?,
                                            );
                                            level_value.insert(
                                                string_value("level"),
                                                serialized_value(*level)?,
                                            );
                                            Ok(Value::Mapping(level_value))
                                        })
                                        .collect::<Result<Vec<_>, ExportProjectError>>()?,
                                ),
                            );
                            Ok(Value::Mapping(item))
                        })
                        .collect::<Result<Vec<_>, ExportProjectError>>()?,
                ),
            );
        }
    }
    Ok(Value::Mapping(value))
}
fn dimming_curve_value(curve: &DimmingCurve) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match curve {
        DimmingCurve::Linear => {
            value.insert(string_value("type"), Value::String("linear".to_string()));
        }
        DimmingCurve::Gamma(gamma) => {
            value.insert(string_value("type"), Value::String("gamma".to_string()));
            value.insert(string_value("value"), serialized_value(*gamma)?);
        }
        DimmingCurve::Custom(curve) => {
            value.insert(string_value("type"), Value::String("custom".to_string()));
            value.insert(string_value("curve"), curve_value(curve)?);
        }
    }
    Ok(Value::Mapping(value))
}
fn function_tag_name(tag: FixtureFunctionTag) -> &'static str {
    match tag {
        FixtureFunctionTag::Pan => "pan",
        FixtureFunctionTag::Tilt => "tilt",
        FixtureFunctionTag::Dimmer => "dimmer",
        FixtureFunctionTag::Shutter => "shutter",
        FixtureFunctionTag::Zoom => "zoom",
        FixtureFunctionTag::Gobo => "gobo",
        FixtureFunctionTag::Frost => "frost",
        FixtureFunctionTag::Prism => "prism",
        FixtureFunctionTag::ColorWheel => "color_wheel",
        FixtureFunctionTag::ColorMixing => "color_mixing",
    }
}
fn entry_tag_name(tag: FixtureEntryTag) -> &'static str {
    match tag {
        FixtureEntryTag::ShutterOpen => "shutter_open",
        FixtureEntryTag::ShutterClosed => "shutter_closed",
        FixtureEntryTag::Strobe => "strobe",
        FixtureEntryTag::PrismOpen => "prism_open",
        FixtureEntryTag::PrismClosed => "prism_closed",
        FixtureEntryTag::GoboOpen => "gobo_open",
    }
}
fn component_name(component: ColorComponent) -> &'static str {
    match component {
        ColorComponent::Red => "red",
        ColorComponent::Green => "green",
        ColorComponent::Blue => "blue",
        ColorComponent::White => "white",
    }
}
