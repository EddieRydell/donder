pub(super) fn sequence_mut<'a>(
    session: &'a mut ProjectSession,
    id: &SequenceId,
) -> Result<&'a mut donder_language::sequence::Sequence, GuiMutationError> {
    session
        .project
        .sequences
        .get_mut(id)
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))
}

pub(super) fn register_sequence_audio_asset(
    session: &mut ProjectSession,
    document: &donder_language::identity::DocumentId,
    import_path: &str,
) -> Result<AssetId, GuiMutationError> {
    if let Some(asset) = session.source.referenced_assets.iter_mut().find(|asset| {
        asset.module_id == document.module_id() && asset.relative_path.as_str() == import_path
    }) {
        asset.referenced_by.insert(document.clone());
        return Ok(asset.id.clone());
    }

    let module = session
        .source
        .module(document.module_id())
        .ok_or_else(|| GuiMutationError::Invalid("Source module was not found.".to_string()))?;
    let selected_path = module.root.join(import_path);
    let absolute_path = fs::canonicalize(&selected_path)
        .map_err(|error| GuiMutationError::Invalid(format!("Audio file was not found: {error}")))?;
    let absolute_path = Utf8PathBuf::from_path_buf(absolute_path).map_err(|path| {
        GuiMutationError::Invalid(format!("Audio path is not valid UTF-8: {}", path.display()))
    })?;
    if !absolute_path.is_file() || !absolute_path.starts_with(&module.root) {
        return Err(GuiMutationError::Invalid(
            "Selected audio path is not a file inside the project module.".to_string(),
        ));
    }

    if let Some(asset) = session.source.referenced_assets.iter().find(|asset| {
        asset.module_id == document.module_id() && asset.absolute_path == absolute_path
    }) {
        return Ok(asset.id.clone());
    }

    let relative_path = Utf8PathBuf::from(import_path);
    let next_id = session
        .source
        .referenced_assets
        .iter()
        .map(|asset| asset.id.0)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let id = AssetId(next_id);
    session.source.referenced_assets.push(ReferencedAsset {
        id: id.clone(),
        module_id: document.module_id(),
        relative_path,
        absolute_path,
        referenced_by: std::collections::BTreeSet::from([document.clone()]),
    });
    Ok(id)
}

pub(super) fn fixture_definition_mut<'a>(
    session: &'a mut ProjectSession,
    identity: &SourceIdentity,
) -> Result<&'a mut donder_language::fixture::FixtureDefinition, GuiMutationError> {
    let id = FixtureDefinitionId(identity.clone());
    session
        .project
        .definitions
        .fixtures
        .definitions
        .get_mut(&id)
        .ok_or_else(|| GuiMutationError::Invalid("Fixture definition was not loaded.".to_string()))
}

pub(super) fn effect_mut(
    sequence: &mut donder_language::sequence::Sequence,
    id: u32,
) -> Result<&mut EffectInst, GuiMutationError> {
    sequence
        .effects
        .iter_mut()
        .find(|effect| effect.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))
}

pub(super) fn composition_graph_node_mut<'a>(
    sequence: &'a mut donder_language::sequence::Sequence,
    id: &CompositionGraphNodeId,
) -> Result<&'a mut CompositionGraphNode, GuiMutationError> {
    sequence
        .composition_graph
        .nodes
        .iter_mut()
        .find(|node| node.id == *id)
        .ok_or_else(|| GuiMutationError::Invalid("Graph node was not found.".to_string()))
}

pub(super) fn parse_graph_node_id(value: &str) -> Result<CompositionGraphNodeId, GuiMutationError> {
    if let Some(id) = value.strip_prefix("node:") {
        return id
            .parse::<u32>()
            .map(CompositionGraphNodeId)
            .map_err(|_| GuiMutationError::Invalid("Invalid graph node id.".to_string()));
    }
    Err(GuiMutationError::Invalid(
        "Invalid graph node id.".to_string(),
    ))
}

pub(super) fn ensure_graph_node_exists(
    sequence: &donder_language::sequence::Sequence,
    node_id: &CompositionGraphNodeId,
) -> Result<(), GuiMutationError> {
    if sequence
        .composition_graph
        .nodes
        .iter()
        .any(|node| node.id == *node_id)
    {
        Ok(())
    } else {
        Err(GuiMutationError::Invalid(
            "Graph node was not found.".to_string(),
        ))
    }
}

pub(super) fn graph_input_cardinality(
    definitions: &donder_language::operator::OperatorDefinitionStore,
    kind: &CompositionGraphNodeKind,
    source_name: &str,
) -> Option<OperatorPortCardinality> {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => None,
        CompositionGraphNodeKind::Operator(operator) => definitions
            .resolve(&operator.operator)?
            .inputs
            .iter()
            .find(|port| port.source_name == source_name)
            .map(|port| port.cardinality.clone()),
        CompositionGraphNodeKind::Output => {
            (source_name == "input").then_some(OperatorPortCardinality::Many)
        }
    }
}

pub(super) fn next_composition_node_id(sequence: &donder_language::sequence::Sequence) -> u32 {
    sequence
        .composition_graph
        .nodes
        .iter()
        .map(|node| node.id.0)
        .max()
        .unwrap_or(0)
        + 1
}

pub(super) fn create_sequence_layer(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    name: String,
    color: String,
    position: Option<(f32, f32)>,
    connect_to_output: bool,
) -> Result<(), GuiMutationError> {
    let sequence = sequence_mut(session, sequence_id)?;
    let next_layer_id = sequence
        .layers
        .iter()
        .map(|layer| layer.id.0)
        .max()
        .unwrap_or(0)
        + 1;
    let output_node_id = sequence
        .composition_graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Output))
        .map(|node| node.id.clone())
        .ok_or_else(|| {
            GuiMutationError::Invalid("Composition graph output was not found.".to_string())
        })?;
    let layer_node_id = CompositionGraphNodeId(next_composition_node_id(sequence));
    sequence
        .layers
        .push(donder_language::sequence::SequenceLayer {
            id: SequenceLayerId(next_layer_id),
            name,
            color: parse_color(&color)?,
            enabled: true,
        });
    let (x, y) = position.unwrap_or((80.0, 120.0 + next_layer_id as f32 * 80.0));
    sequence.composition_graph.nodes.push(CompositionGraphNode {
        id: layer_node_id.clone(),
        position: GraphNodePosition { x, y },
        kind: CompositionGraphNodeKind::Layer {
            layer_id: SequenceLayerId(next_layer_id),
        },
    });
    if connect_to_output {
        sequence.composition_graph.edges.push(EffectGraphEdge {
            from: layer_node_id,
            from_port: GraphPortId("output".to_string()),
            to: output_node_id,
            to_port: GraphPortId("input".to_string()),
        });
    }
    Ok(())
}

pub(super) fn graph_operator_from_gui(
    session: &ProjectSession,
    operator: &SequenceGraphOperator,
) -> Result<OperatorRef, GuiMutationError> {
    Ok(match operator {
        SequenceGraphOperator::Builtin { operator } => OperatorRef::Builtin(match operator {
            SequenceBuiltinOperator::Max => BuiltinOperator::Max,
            SequenceBuiltinOperator::Add => BuiltinOperator::Add,
            SequenceBuiltinOperator::Multiply => BuiltinOperator::Multiply,
            SequenceBuiltinOperator::IntensityModulate => BuiltinOperator::IntensityModulate,
            SequenceBuiltinOperator::Dim => BuiltinOperator::Dim,
            SequenceBuiltinOperator::Invert => BuiltinOperator::Invert,
            SequenceBuiltinOperator::Colorize => BuiltinOperator::Colorize,
            SequenceBuiltinOperator::Delay => BuiltinOperator::Delay,
            SequenceBuiltinOperator::Echo => BuiltinOperator::Echo,
        }),
        SequenceGraphOperator::Custom {
            module_id,
            path,
            object_key,
        } => {
            let identity =
                source_identity_from_gui(module_id, path, identifier(object_key)?.as_str())?;
            if session.source.module(identity.module_id()).is_none() {
                return Err(GuiMutationError::Invalid(
                    "Operator source module was not found.".to_string(),
                ));
            }
            OperatorRef::Custom(OperatorDefinitionId(identity))
        }
    })
}

pub(crate) fn source_identity_from_gui(
    module_id: &str,
    path: &str,
    object: &str,
) -> Result<SourceIdentity, GuiMutationError> {
    let module_id = uuid::Uuid::parse_str(module_id)
        .map_err(|_| GuiMutationError::Invalid("Source module ID is invalid.".to_string()))?;
    Ok(SourceIdentity::from_document(
        donder_language::identity::DocumentId::new(module_id, Utf8PathBuf::from(path)),
        object.to_string(),
    ))
}

pub(super) fn mark_collection_mut<'a>(
    sequence: &'a mut donder_language::sequence::Sequence,
    key: &str,
) -> Result<&'a mut MarkCollection, GuiMutationError> {
    sequence
        .mark_collections
        .iter_mut()
        .find(|collection| collection.key.name == key)
        .ok_or_else(|| GuiMutationError::Invalid("Mark collection was not found.".to_string()))
}

pub(super) fn automation_clip_mut(
    sequence: &mut donder_language::sequence::Sequence,
    id: u32,
) -> Result<&mut AutomationClip, GuiMutationError> {
    sequence
        .automation_clips
        .iter_mut()
        .find(|clip| clip.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Automation clip was not found.".to_string()))
}

pub(super) fn identifier(value: &str) -> Result<Identifier, GuiMutationError> {
    Identifier::new(value.to_string())
        .map_err(|_| GuiMutationError::Invalid(format!("Invalid identifier `{value}`.")))
}

pub(super) fn effect_scope(scope: SequenceEffectScope) -> EffectScope {
    match scope {
        SequenceEffectScope::PerFixture => EffectScope::PerFixture,
        SequenceEffectScope::WholeTarget => EffectScope::WholeTarget,
    }
}

pub(super) fn layout_target_to_effect_target(
    layout: &LayoutId,
    target: FixtureTarget,
) -> DomainFixtureTarget {
    DomainFixtureTarget {
        layout: layout.clone(),
        fixture: FixtureInstanceId(target.fixture),
    }
}

pub(crate) fn effect_param_value_from_gui(
    session: &mut ProjectSession,
    owner: &SourceIdentity,
    value: SequenceEffectParamValue,
) -> Result<EffectParamValue, GuiMutationError> {
    Ok(match value {
        SequenceEffectParamValue::Int { value } => EffectParamValue::Int(value as i32),
        SequenceEffectParamValue::Float { value } => EffectParamValue::Float(value),
        SequenceEffectParamValue::Bool { value } => EffectParamValue::Bool(value),
        SequenceEffectParamValue::Color { value } => EffectParamValue::Color(parse_color(&value)?),
        SequenceEffectParamValue::Enum { value } => EffectParamValue::Enum(identifier(&value)?),
        SequenceEffectParamValue::Marks { key } => {
            EffectParamValue::Marks(MarkCollectionKey { name: key })
        }
        SequenceEffectParamValue::Curve { value } => EffectParamValue::Curve(
            match library_identity(session, owner, SourceObjectKind::Curve, value.source)? {
                Some(id) => CurveSource::Reference(CurveId(id)),
                None => CurveSource::Inline(curve_from_points(value.points)),
            },
        ),
        SequenceEffectParamValue::Gradient { value } => EffectParamValue::Gradient(
            match library_identity(session, owner, SourceObjectKind::Gradient, value.source)? {
                Some(id) => GradientSource::Reference(GradientId(id)),
                None => GradientSource::Inline(gradient_from_stops(value.stops)?),
            },
        ),
        SequenceEffectParamValue::IntArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|value| EffectParamValue::Int(value as i32))
                .collect(),
        ),
        SequenceEffectParamValue::FloatArray { values } => {
            EffectParamValue::Array(values.into_iter().map(EffectParamValue::Float).collect())
        }
        SequenceEffectParamValue::BoolArray { values } => {
            EffectParamValue::Array(values.into_iter().map(EffectParamValue::Bool).collect())
        }
        SequenceEffectParamValue::ColorArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|value| parse_color(&value).map(EffectParamValue::Color))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        SequenceEffectParamValue::CurveArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|value| {
                    effect_param_value_from_gui(
                        session,
                        owner,
                        SequenceEffectParamValue::Curve { value },
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        SequenceEffectParamValue::GradientArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|value| {
                    effect_param_value_from_gui(
                        session,
                        owner,
                        SequenceEffectParamValue::Gradient { value },
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    })
}

fn library_identity(
    session: &mut ProjectSession,
    owner: &SourceIdentity,
    kind: SourceObjectKind,
    source: SequenceLibrarySource,
) -> Result<Option<SourceIdentity>, GuiMutationError> {
    let SequenceLibrarySource::Library {
        module_id,
        path,
        object_key,
        ..
    } = source
    else {
        return Ok(None);
    };
    let id = source_identity_from_gui(&module_id, &path, &object_key)?;
    ensure_document_can_reference_source(session, owner.document_id(), kind, &id)
        .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
    Ok(Some(id))
}

pub(super) fn automation_mapping_from_gui(
    mapping: SequenceAutomationMapping,
) -> Result<AutomationMapping, GuiMutationError> {
    Ok(match mapping {
        SequenceAutomationMapping::Float { min, max } => AutomationMapping::Float { min, max },
        SequenceAutomationMapping::Int { min, max } => AutomationMapping::Int {
            min: min.round() as i32,
            max: max.round() as i32,
        },
        SequenceAutomationMapping::Bool => AutomationMapping::Bool,
        SequenceAutomationMapping::Enum { values } => AutomationMapping::Enum {
            values: values
                .into_iter()
                .map(|value| identifier(&value))
                .collect::<Result<Vec<_>, _>>()?,
        },
        SequenceAutomationMapping::Curve { min, max } => AutomationMapping::Curve { min, max },
    })
}

pub(super) fn automation_binding_value_at(
    clip: &AutomationClip,
    binding: &AutomationBinding,
    seconds: f32,
) -> Result<EffectParamValue, GuiMutationError> {
    automation_value_at(clip, binding, seconds)
        .map(|value| match value {
            AutomationValue::Int(value) => EffectParamValue::Int(value),
            AutomationValue::Float(value) => EffectParamValue::Float(value),
            AutomationValue::Bool(value) => EffectParamValue::Bool(value),
            AutomationValue::Enum(value) => EffectParamValue::Enum(value.clone()),
            AutomationValue::Curve(value) => EffectParamValue::Curve(CurveSource::Inline(value)),
        })
        .ok_or_else(|| {
            GuiMutationError::Invalid("Enum automation mapping has no values.".to_string())
        })
}

pub(super) fn default_automation_curve() -> Curve {
    Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 1.0,
                value: 1.0,
            },
        ],
    }
}

pub(super) fn curve_from_points(points: Vec<SequenceCurvePoint>) -> Curve {
    Curve {
        points: points
            .into_iter()
            .map(|point| CurvePoint {
                position: point.time,
                value: point.value,
            })
            .collect(),
    }
}

pub(super) fn curve_points(curve: &Curve) -> Vec<SequenceCurvePoint> {
    curve
        .points
        .iter()
        .map(|point| SequenceCurvePoint {
            time: point.position,
            value: point.value,
        })
        .collect()
}

pub(super) fn gradient_stops(gradient: &Gradient) -> Vec<SequenceGradientStop> {
    gradient
        .stops
        .iter()
        .map(|stop| SequenceGradientStop {
            time: stop.position,
            value: stop.color.to_hex(),
        })
        .collect()
}

pub(super) fn gradient_from_stops(
    stops: Vec<SequenceGradientStop>,
) -> Result<Gradient, GuiMutationError> {
    Ok(Gradient {
        stops: stops
            .into_iter()
            .map(|stop| {
                Ok(GradientStop {
                    position: stop.time,
                    color: parse_color(&stop.value)?,
                })
            })
            .collect::<Result<Vec<_>, GuiMutationError>>()?,
    })
}

pub(super) fn parse_color(value: &str) -> Result<Color, GuiMutationError> {
    Color::from_hex(value)
        .ok_or_else(|| GuiMutationError::Invalid(format!("Invalid color `{value}`.")))
}

pub(super) fn domain_point3_meters(point: Point3Meters) -> Point3 {
    Point3 {
        x: Distance::from_meters(point.x_meters),
        y: Distance::from_meters(point.y_meters),
        z: Distance::from_meters(point.z_meters),
    }
}

pub(super) fn rotation3_degrees(rotation: Rotation3Degrees) -> DomainRotation3 {
    DomainRotation3 {
        x: rotation.x_degrees,
        y: rotation.y_degrees,
        z: rotation.z_degrees,
    }
}

pub(super) fn scale3(scale: Scale3) -> DomainScale3 {
    DomainScale3 {
        x: scale.x,
        y: scale.y,
        z: scale.z,
    }
}

use std::fs;

use camino::Utf8PathBuf;
use donder_language::dsl::Identifier;
use donder_language::effect::{
    CurveId, CurveSource, EffectInst, EffectParamValue, EffectScope, GradientId, GradientSource,
};
use donder_language::fixture::FixtureDefinitionId;
use donder_language::identity::SourceIdentity;
use donder_language::layout::{FixtureInstanceId, FixtureTarget as DomainFixtureTarget, LayoutId};
use donder_language::operator::{
    BuiltinOperator, OperatorDefinitionId, OperatorPortCardinality, OperatorRef,
};
use donder_language::sequence::{
    AssetId, AutomationBinding, AutomationClip, AutomationMapping, AutomationValue,
    CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
    GraphNodePosition, GraphPortId, MarkCollection, MarkCollectionKey, SequenceId, SequenceLayerId,
    automation_value_at,
};
use donder_language::values::{
    Color, Curve, CurvePoint, Distance, Gradient, GradientStop, Point3,
    Rotation3 as DomainRotation3, Scale3 as DomainScale3,
};
use donder_project_io::{
    ProjectSession, ReferencedAsset, SourceObjectKind, ensure_document_can_reference_source,
};

use super::GuiMutationError;
use crate::dto::{
    FixtureTarget, Point3Meters, Rotation3Degrees, Scale3, SequenceAutomationMapping,
    SequenceBuiltinOperator, SequenceCurvePoint, SequenceEffectParamValue, SequenceEffectScope,
    SequenceGradientStop, SequenceGraphOperator, SequenceLibrarySource,
};
