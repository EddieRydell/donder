pub(super) fn edit_sequence(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: SequenceGuiEdit,
) -> Result<(), GuiMutationError> {
    let add_effect_mark_params = match &edit {
        SequenceGuiEdit::AddEffect {
            effect: effect_reference,
            mark_collection_key: Some(_),
            ..
        } => mark_param_names(session, effect_reference)?,
        _ => Vec::new(),
    };
    let sequence_id = SequenceId(resolved.identity.clone());
    let layout = session
        .project
        .setups
        .get(&session.project.root.setup)
        .map(|setup| setup.layout.clone())
        .ok_or_else(|| GuiMutationError::Invalid("Active layout was not found.".to_string()))?;
    if matches!(
        &edit,
        SequenceGuiEdit::AddEffect { .. }
            | SequenceGuiEdit::RetargetEffect { .. }
            | SequenceGuiEdit::MoveEffect {
                target: Some(_),
                ..
            }
    ) {
        ensure_document_can_reference_source(
            session,
            resolved.identity.document_id(),
            SourceObjectKind::Layout,
            &layout.0,
        )
        .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
    }
    match edit {
        SequenceGuiEdit::SetDuration { duration_seconds } => {
            if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
                return Err(GuiMutationError::Invalid(
                    "Sequence duration must be greater than zero.".to_string(),
                ));
            }
            sequence_mut(session, &sequence_id)?.duration =
                DonderDuration::from_seconds_f32(duration_seconds);
        }
        SequenceGuiEdit::SetAudio { import_path } => {
            let audio = match import_path {
                Some(import_path) => {
                    let id = register_sequence_audio_asset(
                        session,
                        resolved.identity.document_id(),
                        &import_path,
                    )?;
                    DomainSequenceAudio::Asset(id)
                }
                None => DomainSequenceAudio::None,
            };
            sequence_mut(session, &sequence_id)?.audio = audio;
        }
        SequenceGuiEdit::MoveEffect {
            id,
            start_seconds,
            target,
        } => {
            let parsed_target =
                target.map(|target| layout_target_to_effect_target(&layout, target));
            let sequence = sequence_mut(session, &sequence_id)?;
            let effect = effect_mut(sequence, id)?;
            effect.start = DonderTime::from_seconds_f32(start_seconds.max(0.0));
            if let Some(target) = parsed_target {
                effect.target = target;
            }
        }
        SequenceGuiEdit::ResizeEffect {
            id,
            start_seconds,
            duration_seconds,
        } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let start = DonderTime::from_seconds_f32(start_seconds.max(0.0));
            let duration = DonderDuration::from_seconds_f32(duration_seconds.max(0.000000001));
            let effect = effect_mut(sequence, id)?;
            effect.start = start;
            effect.duration = duration;
        }
        SequenceGuiEdit::SetEffectScope { id, scope } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let scope = effect_scope(scope);
            effect_mut(sequence, id)?.scope = scope;
        }
        SequenceGuiEdit::RetargetEffect { id, target } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let target = layout_target_to_effect_target(&layout, target);
            effect_mut(sequence, id)?.target = target;
        }
        SequenceGuiEdit::DeleteEffect { id } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            sequence.effects.retain(|effect| effect.id.0 != id);
            for clip in &mut sequence.automation_clips {
                clip.detach_bindings(AutomationDetachmentReason::TargetDeleted, |target| {
                    matches!(target, AutomationTarget::EffectParam { effect_id, .. } if effect_id.0 == id)
                });
            }
        }
        SequenceGuiEdit::MoveMark {
            collection_key,
            index,
            time_seconds,
        } => {
            let collection =
                mark_collection_mut(sequence_mut(session, &sequence_id)?, &collection_key)?;
            let mark = collection
                .marks
                .get_mut(index as usize)
                .ok_or_else(|| GuiMutationError::Invalid("Mark was not found.".to_string()))?;
            *mark = DonderTime::from_seconds_f32(time_seconds.max(0.0));
            collection.marks.sort_by_key(|time| time.0);
        }
        SequenceGuiEdit::ReassignMarkCollection {
            collection_key,
            index,
            target_collection_key,
        } => {
            if collection_key != target_collection_key {
                let sequence = sequence_mut(session, &sequence_id)?;
                let mark = {
                    let collection = mark_collection_mut(sequence, &collection_key)?;
                    if (index as usize) >= collection.marks.len() {
                        return Err(GuiMutationError::Invalid("Mark was not found.".to_string()));
                    }
                    collection.marks.remove(index as usize)
                };
                let target_collection = mark_collection_mut(sequence, &target_collection_key)?;
                target_collection.marks.push(mark);
                target_collection.marks.sort_by_key(|time| time.0);
            }
        }
        SequenceGuiEdit::AddMark {
            collection_key,
            time_seconds,
        } => {
            let collection =
                mark_collection_mut(sequence_mut(session, &sequence_id)?, &collection_key)?;
            collection
                .marks
                .push(DonderTime::from_seconds_f32(time_seconds.max(0.0)));
            collection.marks.sort_by_key(|time| time.0);
        }
        SequenceGuiEdit::DeleteMark {
            collection_key,
            index,
        } => {
            let collection =
                mark_collection_mut(sequence_mut(session, &sequence_id)?, &collection_key)?;
            if (index as usize) < collection.marks.len() {
                collection.marks.remove(index as usize);
            }
        }
        SequenceGuiEdit::CreateMarkCollection { key, name, color } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            if sequence
                .mark_collections
                .iter()
                .any(|collection| collection.key.name == key)
            {
                return Err(GuiMutationError::Invalid(
                    "Mark collection keys must be unique.".to_string(),
                ));
            }
            sequence.mark_collections.push(MarkCollection {
                key: MarkCollectionKey { name: key },
                name,
                display_color: parse_color(&color)?,
                marks: Vec::new(),
            });
        }
        SequenceGuiEdit::RenameMarkCollection { key, name } => {
            mark_collection_mut(sequence_mut(session, &sequence_id)?, &key)?.name = name;
        }
        SequenceGuiEdit::DeleteMarkCollection { key } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let is_referenced = sequence.effects.iter().any(|effect| {
                effect.param_overrides.values().any(|value| {
                    matches!(value, EffectParamValue::Marks(collection) if collection.name == key)
                })
            });
            if is_referenced {
                return Err(GuiMutationError::Invalid(
                    "Mark collection is still referenced by an effect.".to_string(),
                ));
            }
            sequence
                .mark_collections
                .retain(|collection| collection.key.name != key);
        }
        SequenceGuiEdit::SetMarkCollectionColor { key, color } => {
            mark_collection_mut(sequence_mut(session, &sequence_id)?, &key)?.display_color =
                parse_color(&color)?;
        }
        SequenceGuiEdit::UpdateEffectParam { id, name, value } => {
            let value = effect_param_value_from_gui(session, &resolved.identity, value)?;
            effect_mut(sequence_mut(session, &sequence_id)?, id)?
                .param_overrides
                .insert(identifier(&name)?, value);
        }
        SequenceGuiEdit::AddEffect {
            effect: effect_reference,
            target,
            initial_color,
            scope,
            start_seconds,
            mark_collection_key,
        } => {
            let initial_color = parse_color(&initial_color)?;
            let definition = effect_ref_from_gui(session, effect_reference)?;
            let Some(effect_definition) = session.project.definitions.effects.resolve(&definition)
            else {
                return Err(GuiMutationError::Invalid(
                    "Effect was not found.".to_string(),
                ));
            };
            let params = effect_definition.params.clone();
            if let EffectRef::Custom(definition) = &definition {
                ensure_document_can_reference_source(
                    session,
                    resolved.identity.document_id(),
                    SourceObjectKind::EffectDefinition,
                    &definition.0,
                )
                .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
            }
            let sequence = sequence_mut(session, &sequence_id)?;
            let layer_id = sequence
                .layers
                .first()
                .map(|layer| layer.id.clone())
                .ok_or_else(|| {
                    GuiMutationError::Invalid(
                        "An effect cannot be added to a sequence without a layer.".to_string(),
                    )
                })?;
            let next_id = sequence
                .effects
                .iter()
                .map(|effect| effect.id.0)
                .max()
                .unwrap_or(0)
                + 1;
            let mut param_overrides = IndexMap::new();
            if let Some(key) = mark_collection_key {
                for name in add_effect_mark_params {
                    param_overrides.insert(
                        identifier(&name)?,
                        EffectParamValue::Marks(MarkCollectionKey { name: key.clone() }),
                    );
                }
            }
            for param in params.iter().filter(|param| param.default.is_none()) {
                if param_overrides.contains_key(&param.name) {
                    continue;
                }
                let value = EffectParamValue::initial_for_type(&param.ty, initial_color)
                    .ok_or_else(|| {
                        GuiMutationError::Invalid(format!(
                            "Effect parameter `{}` requires an explicit value.",
                            param.name.as_str()
                        ))
                    })?;
                param_overrides.insert(param.name.clone(), value);
            }
            sequence.effects.push(EffectInst {
                id: EffectInstId(next_id),
                layer_id,
                start: DonderTime::from_seconds_f32(start_seconds.max(0.0)),
                duration: DonderDuration::from_seconds_f32(1.0),
                target: layout_target_to_effect_target(&layout, target),
                scope: effect_scope(scope),
                definition,
                param_overrides,
            });
        }
        SequenceGuiEdit::CreateLayer { name, color } => {
            create_sequence_layer(session, &sequence_id, name, color, None, true)?;
        }
        SequenceGuiEdit::CreateLayerAt { name, color, x, y } => {
            create_sequence_layer(session, &sequence_id, name, color, Some((x, y)), false)?;
        }
        SequenceGuiEdit::RenameLayer { id, name } => {
            let layer = sequence_mut(session, &sequence_id)?
                .layers
                .iter_mut()
                .find(|layer| layer.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))?;
            layer.name = name;
        }
        SequenceGuiEdit::SetLayerColor { id, color } => {
            let layer = sequence_mut(session, &sequence_id)?
                .layers
                .iter_mut()
                .find(|layer| layer.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))?;
            layer.color = parse_color(&color)?;
        }
        SequenceGuiEdit::SetLayerEnabled { id, enabled } => {
            let layer = sequence_mut(session, &sequence_id)?
                .layers
                .iter_mut()
                .find(|layer| layer.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))?;
            layer.enabled = enabled;
        }
        SequenceGuiEdit::DeleteLayer {
            id,
            migrate_to_layer_id,
        } => {
            if id == 0 {
                return Err(GuiMutationError::Invalid(
                    "Default layer cannot be deleted.".to_string(),
                ));
            }
            let sequence = sequence_mut(session, &sequence_id)?;
            if !sequence.layers.iter().any(|layer| layer.id.0 == id) {
                return Err(GuiMutationError::Invalid(
                    "Layer was not found.".to_string(),
                ));
            }
            if migrate_to_layer_id == id
                || !sequence
                    .layers
                    .iter()
                    .any(|layer| layer.id.0 == migrate_to_layer_id)
            {
                return Err(GuiMutationError::Invalid(
                    "Effect migration layer was not found.".to_string(),
                ));
            }
            for effect in &mut sequence.effects {
                if effect.layer_id.0 == id {
                    effect.layer_id = SequenceLayerId(migrate_to_layer_id);
                }
            }
            sequence.layers.retain(|layer| layer.id.0 != id);
            let layer_node_ids = sequence
                .composition_graph
                .nodes
                .iter()
                .filter_map(|node| match &node.kind {
                    CompositionGraphNodeKind::Layer { layer_id } if layer_id.0 == id => {
                        Some(node.id.clone())
                    }
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            sequence
                .composition_graph
                .nodes
                .retain(|node| !layer_node_ids.contains(&node.id));
            sequence.composition_graph.edges.retain(|edge| {
                !layer_node_ids.contains(&edge.from) && !layer_node_ids.contains(&edge.to)
            });
        }
        SequenceGuiEdit::SetEffectLayer { id, layer_id } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            if !sequence.layers.iter().any(|layer| layer.id.0 == layer_id) {
                return Err(GuiMutationError::Invalid(
                    "Layer was not found.".to_string(),
                ));
            }
            let effect = sequence
                .effects
                .iter_mut()
                .find(|effect| effect.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))?;
            effect.layer_id = SequenceLayerId(layer_id);
        }
        SequenceGuiEdit::ChangeEffectDefinition {
            id,
            effect: effect_reference,
            initial_color,
        } => {
            let initial_color = parse_color(&initial_color)?;
            let definition = effect_ref_from_gui(session, effect_reference)?;
            let Some(effect_definition) = session.project.definitions.effects.resolve(&definition)
            else {
                return Err(GuiMutationError::Invalid(
                    "Effect was not found.".to_string(),
                ));
            };
            let params = effect_definition.params.clone();
            let mut param_overrides = IndexMap::new();
            for param in params.iter().filter(|param| param.default.is_none()) {
                let value = EffectParamValue::initial_for_type(&param.ty, initial_color).ok_or_else(|| {
                    GuiMutationError::Invalid(format!(
                        "Effect parameter `{}` requires an explicit value before changing scripts.",
                        param.name.as_str()
                    ))
                })?;
                param_overrides.insert(param.name.clone(), value);
            }
            if let EffectRef::Custom(definition) = &definition {
                ensure_document_can_reference_source(
                    session,
                    resolved.identity.document_id(),
                    SourceObjectKind::EffectDefinition,
                    &definition.0,
                )
                .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
            }
            let sequence = sequence_mut(session, &sequence_id)?;
            let effect = effect_mut(sequence, id)?;
            effect.definition = definition;
            effect.param_overrides = param_overrides;
            for clip in &mut sequence.automation_clips {
                clip.detach_bindings(AutomationDetachmentReason::DefinitionChanged, |target| {
                    matches!(target, AutomationTarget::EffectParam { effect_id, .. } if effect_id.0 == id)
                });
            }
        }
        SequenceGuiEdit::AddGraphOperatorNode {
            operator,
            x,
            y,
            initial_color,
        } => {
            let initial_color = parse_color(&initial_color)?;
            let operator = graph_operator_from_gui(session, &operator)?;
            let definition = session
                .project
                .definitions
                .operators
                .resolve(&operator)
                .cloned()
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Operator definition was not found.".to_string())
                })?;
            if let OperatorRef::Custom(id) = &operator {
                ensure_document_can_reference_source(
                    session,
                    resolved.identity.document_id(),
                    SourceObjectKind::OperatorDefinition,
                    &id.0,
                )
                .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
            }
            let sequence = sequence_mut(session, &sequence_id)?;
            let mut params = IndexMap::new();
            for declaration in &definition.params {
                if declaration.default.is_none() {
                    let value = required_operator_param_value(
                        declaration.ty.clone(),
                        sequence,
                        initial_color,
                    )?;
                    params.insert(declaration.name.clone(), value);
                }
            }
            let next_id = next_composition_node_id(sequence);
            sequence.composition_graph.nodes.push(CompositionGraphNode {
                id: CompositionGraphNodeId(next_id),
                position: GraphNodePosition { x, y },
                kind: CompositionGraphNodeKind::Operator(GraphOperatorNode { operator, params }),
            });
        }
        SequenceGuiEdit::MoveGraphNode { node_id, x, y } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let node_id = parse_graph_node_id(&node_id)?;
            let node = composition_graph_node_mut(sequence, &node_id)?;
            node.position = GraphNodePosition { x, y };
        }
        SequenceGuiEdit::DeleteGraphNode { node_id } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let node_id = parse_graph_node_id(&node_id)?;
            let node = sequence
                .composition_graph
                .nodes
                .iter()
                .find(|node| node.id == node_id)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Graph node was not found.".to_string())
                })?;
            match &node.kind {
                CompositionGraphNodeKind::Layer { .. } => {
                    return Err(GuiMutationError::Invalid(
                        "Delete the layer from the layer list.".to_string(),
                    ));
                }
                CompositionGraphNodeKind::Output => {
                    return Err(GuiMutationError::Invalid(
                        "Output node cannot be deleted.".to_string(),
                    ));
                }
                CompositionGraphNodeKind::Operator(_) => {}
            }
            sequence
                .composition_graph
                .nodes
                .retain(|node| node.id != node_id);
            sequence
                .composition_graph
                .edges
                .retain(|edge| edge.from != node_id && edge.to != node_id);
            for clip in &mut sequence.automation_clips {
                clip.detach_bindings(AutomationDetachmentReason::TargetDeleted, |target| {
                    matches!(target, AutomationTarget::CompositionNodeParam { node_id: binding_node_id, .. } if binding_node_id == &node_id)
                });
            }
        }
        SequenceGuiEdit::ConnectGraphNodes {
            from_node,
            from_port,
            to_node,
            to_port,
        } => {
            if from_node == to_node {
                return Err(GuiMutationError::Invalid(
                    "Graph node cannot connect to itself.".to_string(),
                ));
            }
            let definitions = session.project.definitions.operators.clone();
            let sequence = sequence_mut(session, &sequence_id)?;
            let from = parse_graph_node_id(&from_node)?;
            let to = parse_graph_node_id(&to_node)?;
            ensure_graph_node_exists(sequence, &from)?;
            ensure_graph_node_exists(sequence, &to)?;
            if sequence.composition_graph.edges.iter().any(|edge| {
                edge.from == from
                    && edge.from_port.0 == from_port
                    && edge.to == to
                    && edge.to_port.0 == to_port
            }) {
                return Ok(());
            }
            let mut graph = sequence.composition_graph.clone();
            let single_input = graph
                .nodes
                .iter()
                .find(|node| node.id == to)
                .and_then(|node| graph_input_cardinality(&definitions, &node.kind, &to_port))
                == Some(OperatorPortCardinality::One);
            if single_input {
                graph
                    .edges
                    .retain(|edge| edge.to != to || edge.to_port.0 != to_port);
            }
            graph.edges.push(EffectGraphEdge {
                from,
                from_port: GraphPortId(from_port),
                to,
                to_port: GraphPortId(to_port),
            });
            validate_composition_graph(&graph, &definitions)
                .map_err(|error| GuiMutationError::Invalid(error.message))?;
            sequence.composition_graph = graph;
        }
        SequenceGuiEdit::DisconnectGraphNodes {
            from_node,
            from_port,
            to_node,
            to_port,
        } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let from = parse_graph_node_id(&from_node)?;
            let to = parse_graph_node_id(&to_node)?;
            sequence.composition_graph.edges.retain(|edge| {
                !(edge.from == from
                    && edge.from_port.0 == from_port
                    && edge.to == to
                    && edge.to_port.0 == to_port)
            });
        }
        SequenceGuiEdit::UpdateGraphOperatorParam {
            node_id,
            name,
            value,
        } => {
            let value = effect_param_value_from_gui(session, &resolved.identity, value)?;
            let definitions = session.project.definitions.operators.clone();
            let sequence = sequence_mut(session, &sequence_id)?;
            let node_id = parse_graph_node_id(&node_id)?;
            let mut graph = sequence.composition_graph.clone();
            let node = graph
                .nodes
                .iter_mut()
                .find(|node| node.id == node_id)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Graph node was not found.".to_string())
                })?;
            let CompositionGraphNodeKind::Operator(operator) = &mut node.kind else {
                return Err(GuiMutationError::Invalid(
                    "Graph node is not an operator.".to_string(),
                ));
            };
            operator.params.insert(identifier(&name)?, value);
            validate_composition_graph(&graph, &definitions)
                .map_err(|error| GuiMutationError::Invalid(error.message))?;
            sequence.composition_graph = graph;
        }
        SequenceGuiEdit::AddAutomationClip {
            start_seconds,
            duration_seconds,
            anchor_lane_index,
            lane_index,
        } => {
            let sequence = sequence_mut(session, &sequence_id)?;
            let next_id = sequence
                .automation_clips
                .iter()
                .map(|clip| clip.id.0)
                .max()
                .unwrap_or(0)
                + 1;
            sequence.automation_clips.push(AutomationClip {
                id: AutomationClipId(next_id),
                start: DonderTime::from_seconds_f32(start_seconds.max(0.0)),
                duration: DonderDuration::from_seconds_f32(duration_seconds.max(0.000000001)),
                anchor_lane_index,
                lane_index,
                curve: default_automation_curve(),
                bindings: Vec::new(),
                detached_bindings: Vec::new(),
            });
        }
        SequenceGuiEdit::CreateAndBindAutomationClip { target, mapping } => {
            let target = automation_target_from_gui(target)?;
            donder_language::validation::automation_target_type(
                &session.project,
                session.project.sequences.get(&sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?,
                &target,
            )
            .map_err(|error| GuiMutationError::Invalid(error.message))?;
            let mapping = automation_mapping_from_gui(mapping)?;
            let (start, duration, anchor_lane_index) = {
                let sequence = session.project.sequences.get(&sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?;
                automation_target_timing(session, sequence, &target)?
            };
            let sequence = sequence_mut(session, &sequence_id)?;
            ensure_automation_target_available(sequence, &target, None)?;
            let next_id = sequence
                .automation_clips
                .iter()
                .map(|clip| clip.id.0)
                .max()
                .unwrap_or(0)
                + 1;
            sequence.automation_clips.push(AutomationClip {
                id: AutomationClipId(next_id),
                start,
                duration,
                anchor_lane_index,
                lane_index: 0,
                curve: default_automation_curve(),
                bindings: vec![AutomationBinding { target, mapping }],
                detached_bindings: Vec::new(),
            });
        }
        SequenceGuiEdit::MoveAutomationClip {
            id,
            start_seconds,
            anchor_lane_index,
            lane_index,
        } => {
            let clip = automation_clip_mut(sequence_mut(session, &sequence_id)?, id)?;
            clip.start = DonderTime::from_seconds_f32(start_seconds.max(0.0));
            clip.anchor_lane_index = anchor_lane_index;
            clip.lane_index = lane_index;
        }
        SequenceGuiEdit::ResizeAutomationClip {
            id,
            start_seconds,
            duration_seconds,
        } => {
            let clip = automation_clip_mut(sequence_mut(session, &sequence_id)?, id)?;
            clip.start = DonderTime::from_seconds_f32(start_seconds.max(0.0));
            clip.duration = DonderDuration::from_seconds_f32(duration_seconds.max(0.000000001));
        }
        SequenceGuiEdit::UpdateAutomationCurve { id, curve } => {
            automation_clip_mut(sequence_mut(session, &sequence_id)?, id)?.curve =
                curve_from_points(curve);
        }
        SequenceGuiEdit::UpdateAutomationParamMapping {
            clip_id,
            target,
            mapping,
        } => {
            let target = automation_target_from_gui(target)?;
            donder_language::validation::automation_target_type(
                &session.project,
                session.project.sequences.get(&sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?,
                &target,
            )
            .map_err(|error| GuiMutationError::Invalid(error.message))?;
            let binding = automation_clip_mut(sequence_mut(session, &sequence_id)?, clip_id)?
                .bindings
                .iter_mut()
                .find(|binding| binding.target == target)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Automation binding was not found.".to_string())
                })?;
            binding.mapping = automation_mapping_from_gui(mapping)?;
        }
        SequenceGuiEdit::DeleteAutomationClip { id } => {
            sequence_mut(session, &sequence_id)?
                .automation_clips
                .retain(|clip| clip.id.0 != id);
        }
        SequenceGuiEdit::BindAutomationParam {
            clip_id,
            target,
            mapping,
        } => {
            let target = automation_target_from_gui(target)?;
            donder_language::validation::automation_target_type(
                &session.project,
                session.project.sequences.get(&sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?,
                &target,
            )
            .map_err(|error| GuiMutationError::Invalid(error.message))?;
            let sequence = sequence_mut(session, &sequence_id)?;
            ensure_automation_target_available(sequence, &target, Some(clip_id))?;
            automation_clip_mut(sequence, clip_id)?
                .bind(target, automation_mapping_from_gui(mapping)?);
        }
        SequenceGuiEdit::UnbindAutomationParam { clip_id, target } => {
            let target = automation_target_from_gui(target)?;
            let sequence = sequence_mut(session, &sequence_id)?;
            let clip = sequence
                .automation_clips
                .iter()
                .find(|clip| clip.id.0 == clip_id)
                .cloned()
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Automation clip was not found.".to_string())
                })?;
            let Some(binding) = clip
                .bindings
                .iter()
                .find(|binding| binding.target == target)
                .cloned()
            else {
                return Err(GuiMutationError::Invalid(
                    "Automation binding was not found.".to_string(),
                ));
            };
            let sample_seconds = match &target {
                AutomationTarget::EffectParam { effect_id, .. } => sequence
                    .effects
                    .iter()
                    .find(|effect| &effect.id == effect_id)
                    .map(|effect| effect.start.as_seconds_f32())
                    .ok_or_else(|| {
                        GuiMutationError::Invalid("Effect was not found.".to_string())
                    })?,
                AutomationTarget::CompositionNodeParam { .. } => clip.start.as_seconds_f32(),
            };
            let value = automation_binding_value_at(&clip, &binding, sample_seconds)?;
            match &target {
                AutomationTarget::EffectParam { effect_id, param } => {
                    effect_mut(sequence, effect_id.0)?
                        .param_overrides
                        .insert(param.clone(), value);
                }
                AutomationTarget::CompositionNodeParam { node_id, param } => {
                    let node = composition_graph_node_mut(sequence, node_id)?;
                    let CompositionGraphNodeKind::Operator(operator) = &mut node.kind else {
                        return Err(GuiMutationError::Invalid(
                            "Automation graph node is not an operator.".to_string(),
                        ));
                    };
                    operator.params.insert(param.clone(), value);
                }
            }
            automation_clip_mut(sequence, clip_id)?
                .bindings
                .retain(|binding| binding.target != target);
        }
        SequenceGuiEdit::RebindDetachedAutomation {
            clip_id,
            detached_index,
            target,
            mapping,
        } => {
            let target = automation_target_from_gui(target)?;
            donder_language::validation::automation_target_type(
                &session.project,
                session.project.sequences.get(&sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?,
                &target,
            )
            .map_err(|error| GuiMutationError::Invalid(error.message))?;
            let mapping = automation_mapping_from_gui(mapping)?;
            let sequence = sequence_mut(session, &sequence_id)?;
            ensure_automation_target_available(sequence, &target, Some(clip_id))?;
            let clip = automation_clip_mut(sequence, clip_id)?;
            if detached_index as usize >= clip.detached_bindings.len() {
                return Err(GuiMutationError::Invalid(
                    "Detached automation binding was not found.".to_string(),
                ));
            }
            clip.detached_bindings.remove(detached_index as usize);
            clip.bind(target, mapping);
        }
        SequenceGuiEdit::DiscardDetachedAutomation {
            clip_id,
            detached_index,
        } => {
            let clip = automation_clip_mut(sequence_mut(session, &sequence_id)?, clip_id)?;
            if detached_index as usize >= clip.detached_bindings.len() {
                return Err(GuiMutationError::Invalid(
                    "Detached automation binding was not found.".to_string(),
                ));
            }
            clip.detached_bindings.remove(detached_index as usize);
        }
    }
    Ok(())
}

fn automation_target_from_gui(
    target: SequenceAutomationTarget,
) -> Result<AutomationTarget, GuiMutationError> {
    Ok(match target {
        SequenceAutomationTarget::EffectParam { effect_id, param } => {
            AutomationTarget::EffectParam {
                effect_id: EffectInstId(effect_id),
                param: identifier(&param)?,
            }
        }
        SequenceAutomationTarget::CompositionNodeParam { node_id, param } => {
            AutomationTarget::CompositionNodeParam {
                node_id: parse_graph_node_id(&node_id)?,
                param: identifier(&param)?,
            }
        }
    })
}

fn automation_target_timing(
    session: &ProjectSession,
    sequence: &donder_language::sequence::Sequence,
    target: &AutomationTarget,
) -> Result<(DonderTime, DonderDuration, u32), GuiMutationError> {
    match target {
        AutomationTarget::EffectParam { effect_id, .. } => {
            let effect = sequence
                .effects
                .iter()
                .find(|effect| &effect.id == effect_id)
                .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))?;
            let anchor_lane_index = effect_lane_index_resolved(session, &effect.target)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Effect lane was not found.".to_string())
                })?;
            Ok((
                effect.start.clone(),
                effect.duration.clone(),
                anchor_lane_index as u32,
            ))
        }
        AutomationTarget::CompositionNodeParam { node_id, .. } => {
            let node = sequence
                .composition_graph
                .nodes
                .iter()
                .find(|node| &node.id == node_id)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Automation graph node is missing.".to_string())
                })?;
            if !matches!(node.kind, CompositionGraphNodeKind::Operator(_)) {
                return Err(GuiMutationError::Invalid(
                    "Automation graph node is not an operator.".to_string(),
                ));
            }
            if sequence.layers.is_empty() {
                return Err(GuiMutationError::Invalid(
                    "Sequence has no lane for automation.".to_string(),
                ));
            }
            Ok((
                DonderTime::from_seconds_f32(0.0),
                sequence.duration.clone(),
                0,
            ))
        }
    }
}

fn ensure_automation_target_available(
    sequence: &donder_language::sequence::Sequence,
    target: &AutomationTarget,
    binding_clip_id: Option<u32>,
) -> Result<(), GuiMutationError> {
    if sequence.automation_clips.iter().any(|clip| {
        clip.bindings
            .iter()
            .any(|binding| &binding.target == target)
            || (binding_clip_id != Some(clip.id.0)
                && clip
                    .detached_bindings
                    .iter()
                    .any(|binding| &binding.target == target))
    }) {
        return Err(GuiMutationError::Invalid(
            "Param is already automated.".to_string(),
        ));
    }
    Ok(())
}

fn effect_ref_from_gui(
    session: &ProjectSession,
    reference: SequenceEffectReference,
) -> Result<EffectRef, GuiMutationError> {
    Ok(match reference {
        SequenceEffectReference::Builtin { effect } => EffectRef::Builtin(match effect {
            SequenceBuiltinEffect::Pulse => BuiltinEffect::Pulse,
            SequenceBuiltinEffect::Chase => BuiltinEffect::Chase,
            SequenceBuiltinEffect::Spin => BuiltinEffect::Spin,
            SequenceBuiltinEffect::MarkPulse => BuiltinEffect::MarkPulse,
            SequenceBuiltinEffect::MarkChase => BuiltinEffect::MarkChase,
        }),
        SequenceEffectReference::Custom {
            module_id,
            path,
            effect_name,
        } => {
            let identity = source_identity_from_gui(&module_id, &path, &effect_name)?;
            if session.source.module(identity.module_id()).is_none() {
                return Err(GuiMutationError::Invalid(
                    "Effect source module was not found.".to_string(),
                ));
            }
            EffectRef::Custom(EffectDefinitionId(identity))
        }
    })
}

use std::collections::BTreeSet;

use donder_language::effect::{
    BuiltinEffect, EffectDefinitionId, EffectInst, EffectInstId, EffectParamValue, EffectRef,
};
use donder_language::operator::{
    GraphOperatorNode, OperatorPortCardinality, OperatorRef, validate_composition_graph,
};
use donder_language::sequence::{
    AutomationBinding, AutomationClip, AutomationClipId, AutomationDetachmentReason,
    AutomationTarget, CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind,
    EffectGraphEdge, GraphNodePosition, GraphPortId, MarkCollection, MarkCollectionKey,
    SequenceAudio as DomainSequenceAudio, SequenceId, SequenceLayerId,
};
use donder_language::values::{DonderDuration, DonderTime};
use donder_project_io::{ProjectSession, SourceObjectKind, ensure_document_can_reference_source};
use indexmap::IndexMap;

use super::model::{
    automation_binding_value_at, automation_clip_mut, automation_mapping_from_gui,
    composition_graph_node_mut, create_sequence_layer, curve_from_points, default_automation_curve,
    effect_mut, effect_param_value_from_gui, effect_scope, ensure_graph_node_exists,
    graph_input_cardinality, graph_operator_from_gui, identifier, layout_target_to_effect_target,
    mark_collection_mut, next_composition_node_id, parse_color, parse_graph_node_id,
    register_sequence_audio_asset, sequence_mut, source_identity_from_gui,
};
use super::selection::{
    effect_lane_index_resolved, mark_param_names, required_operator_param_value,
};
use super::{GuiMutationError, ResolvedGuiObject};
use crate::dto::{
    SequenceAutomationTarget, SequenceBuiltinEffect, SequenceEffectReference, SequenceGuiEdit,
};
