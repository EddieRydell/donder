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
    let sequence_id = SequenceId(resolved.object_identity());
    let mut draft = session
        .project
        .sequence(&sequence_id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".into()))?;
    let layout = session
        .project
        .setup(session.project.root().setup.id())
        .map(|setup| setup.layout.id().clone())
        .ok_or_else(|| GuiMutationError::Invalid("Active layout was not found.".to_string()))?;
    if matches!(
        &edit,
        SequenceGuiEdit::AddEffect { .. }
            | SequenceGuiEdit::AddAutomationClip { .. }
            | SequenceGuiEdit::MoveAutomationClip { .. }
            | SequenceGuiEdit::CreateAndBindAutomationClip { .. }
            | SequenceGuiEdit::RetargetEffect { .. }
            | SequenceGuiEdit::MoveEffect {
                target: Some(_),
                ..
            }
    ) {
        donder_project_io::ensure_document_can_reference_object(
            session,
            resolved.identity.document_id(),
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
            draft.duration = super::checked_gui_duration(duration_seconds)?;
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
            draft.audio = audio;
        }
        SequenceGuiEdit::MoveEffect {
            id,
            start_seconds,
            target,
        } => {
            let parsed_target =
                target.map(|target| layout_target_to_effect_target(&layout, target));
            let sequence = &mut draft;
            let effect = effect_mut(sequence, id)?;
            effect.start = super::checked_gui_time(start_seconds.max(0.0))?;
            if let Some(target) = parsed_target {
                effect.target = target;
            }
        }
        SequenceGuiEdit::ResizeEffect {
            id,
            start_seconds,
            duration_seconds,
        } => {
            let sequence = &mut draft;
            let start = super::checked_gui_time(start_seconds.max(0.0))?;
            let duration = super::checked_gui_duration(duration_seconds.max(0.000000001))?;
            let effect = effect_mut(sequence, id)?;
            effect.start = start;
            effect.duration = duration;
        }
        SequenceGuiEdit::SetEffectScope { id, scope } => {
            let sequence = &mut draft;
            let scope = effect_scope(scope);
            effect_mut(sequence, id)?.scope = scope;
        }
        SequenceGuiEdit::RetargetEffect { id, target } => {
            let sequence = &mut draft;
            let target = layout_target_to_effect_target(&layout, target);
            effect_mut(sequence, id)?.target = target;
        }
        SequenceGuiEdit::DeleteEffect { id } => {
            let sequence = &mut draft;
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
            let collection = mark_collection_mut(&mut draft, &collection_key)?;
            let mark = collection
                .marks
                .get_mut(index as usize)
                .ok_or_else(|| GuiMutationError::Invalid("Mark was not found.".to_string()))?;
            *mark = super::checked_gui_time(time_seconds.max(0.0))?;
            collection.marks.sort_by_key(|time| time.0);
        }
        SequenceGuiEdit::ReassignMarkCollection {
            collection_key,
            index,
            target_collection_key,
        } => {
            if collection_key != target_collection_key {
                let sequence = &mut draft;
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
            let collection = mark_collection_mut(&mut draft, &collection_key)?;
            collection
                .marks
                .push(super::checked_gui_time(time_seconds.max(0.0))?);
            collection.marks.sort_by_key(|time| time.0);
        }
        SequenceGuiEdit::DeleteMark {
            collection_key,
            index,
        } => {
            let collection = mark_collection_mut(&mut draft, &collection_key)?;
            if (index as usize) < collection.marks.len() {
                collection.marks.remove(index as usize);
            }
        }
        SequenceGuiEdit::CreateMarkCollection { key, name, color } => {
            let sequence = &mut draft;
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
            mark_collection_mut(&mut draft, &key)?.name = name;
        }
        SequenceGuiEdit::DeleteMarkCollection { key } => {
            let sequence = &mut draft;
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
            mark_collection_mut(&mut draft, &key)?.display_color = parse_color(&color)?;
        }
        SequenceGuiEdit::UpdateEffectParam { id, name, value } => {
            let value = effect_param_value_from_gui(session, &resolved.identity, value)?;
            effect_mut(&mut draft, id)?
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
            let Some(effect_definition) =
                session.project.definitions().effects.resolve(&definition)
            else {
                return Err(GuiMutationError::Invalid(
                    "Effect was not found.".to_string(),
                ));
            };
            let params = effect_definition.params().to_vec();
            let EffectRef::Custom(definition_id) = &definition;
            ensure_document_can_reference_source(
                session,
                resolved.identity.document_id(),
                SourceObjectKind::EffectDefinition,
                &definition_id.0,
            )
            .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
            let sequence = &mut draft;
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
                start: super::checked_gui_time(start_seconds.max(0.0))?,
                duration: super::checked_gui_duration(1.0)?,
                target: layout_target_to_effect_target(&layout, target),
                scope: effect_scope(scope),
                definition,
                param_overrides,
            });
        }
        SequenceGuiEdit::CreateLayer { name, color } => {
            create_sequence_layer(&mut draft, name, color, None, true)?;
        }
        SequenceGuiEdit::CreateLayerAt { name, color, x, y } => {
            create_sequence_layer(&mut draft, name, color, Some((x, y)), false)?;
        }
        SequenceGuiEdit::RenameLayer { id, name } => {
            let layer = draft
                .layers
                .iter_mut()
                .find(|layer| layer.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))?;
            layer.name = name;
        }
        SequenceGuiEdit::SetLayerColor { id, color } => {
            let layer = draft
                .layers
                .iter_mut()
                .find(|layer| layer.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))?;
            layer.color = parse_color(&color)?;
        }
        SequenceGuiEdit::SetLayerEnabled { id, enabled } => {
            let layer = draft
                .layers
                .iter_mut()
                .find(|layer| layer.id.0 == id)
                .ok_or_else(|| GuiMutationError::Invalid("Layer was not found.".to_string()))?;
            layer.enabled = enabled;
        }
        SequenceGuiEdit::SetEffectLayer { id, layer_id } => {
            let sequence = &mut draft;
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
            let Some(effect_definition) =
                session.project.definitions().effects.resolve(&definition)
            else {
                return Err(GuiMutationError::Invalid(
                    "Effect was not found.".to_string(),
                ));
            };
            let params = effect_definition.params().to_vec();
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
            let EffectRef::Custom(definition_id) = &definition;
            ensure_document_can_reference_source(
                session,
                resolved.identity.document_id(),
                SourceObjectKind::EffectDefinition,
                &definition_id.0,
            )
            .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
            let sequence = &mut draft;
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
                .definitions()
                .operators
                .resolve(&operator)
                .cloned()
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Operator definition was not found.".to_string())
                })?;
            let OperatorRef::Custom(id) = &operator;
            ensure_document_can_reference_source(
                session,
                resolved.identity.document_id(),
                SourceObjectKind::OperatorDefinition,
                &id.0,
            )
            .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
            let sequence = &mut draft;
            let mut params = IndexMap::new();
            for declaration in definition.params() {
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
        SequenceGuiEdit::MoveGraphNodes { positions } => {
            super::graph::move_nodes(&mut draft, positions)?;
        }
        SequenceGuiEdit::DeleteGraphItems {
            node_ids,
            layer_ids,
            edges,
            migrate_to_layer_id,
        } => {
            super::graph::delete_items(
                &mut draft,
                node_ids,
                layer_ids,
                edges,
                migrate_to_layer_id,
            )?;
        }
        SequenceGuiEdit::ConnectGraphNodes {
            from_node,
            from_port,
            to_node,
            to_port,
        } => {
            let definitions = session.project.definitions().operators.clone();
            super::graph::connect_nodes(
                &mut draft,
                &definitions,
                crate::dto::SequenceGraphEdge {
                    from_node,
                    from_port,
                    to_node,
                    to_port,
                },
                None,
            )?;
        }
        SequenceGuiEdit::ReconnectGraphEdge {
            previous,
            connection,
        } => {
            let definitions = session.project.definitions().operators.clone();
            super::graph::connect_nodes(&mut draft, &definitions, connection, Some(previous))?;
        }
        SequenceGuiEdit::UpdateGraphOperatorParam {
            node_id,
            name,
            value,
        } => {
            let value = effect_param_value_from_gui(session, &resolved.identity, value)?;
            let definitions = session.project.definitions().operators.clone();
            let sequence = &mut draft;
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
            row_target,
        } => {
            let sequence = &mut draft;
            let next_id = sequence
                .automation_clips
                .iter()
                .map(|clip| clip.id.0)
                .max()
                .unwrap_or(0)
                + 1;
            sequence.automation_clips.push(AutomationClip {
                id: AutomationClipId(next_id),
                start: super::checked_gui_time(start_seconds.max(0.0))?,
                duration: super::checked_gui_duration(duration_seconds.max(0.000000001))?,
                row_target: layout_target_to_effect_target(&layout, row_target),
                curve: default_automation_curve(),
                bindings: Vec::new(),
                detached_bindings: Vec::new(),
            });
        }
        SequenceGuiEdit::CreateAndBindAutomationClip { target, mapping } => {
            let target = automation_target_from_gui(target)?;
            donder_language::validation::automation_target_type(
                &session.project,
                session.project.sequence(&sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?,
                &target,
            )
            .map_err(|error| GuiMutationError::Invalid(error.message))?;
            let mapping = automation_mapping_from_gui(mapping)?;
            let (start, duration, row_target) = {
                let sequence = session.project.sequence(&sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?;
                automation_target_timing(session, sequence, &target)?
            };
            let next_id = draft
                .automation_clips
                .iter()
                .map(|clip| clip.id.0)
                .max()
                .unwrap_or(0)
                + 1;
            let mut clip = AutomationClip {
                id: AutomationClipId(next_id),
                start,
                duration,
                row_target,
                curve: default_automation_curve(),
                bindings: Vec::new(),
                detached_bindings: Vec::new(),
            };
            let mapping = available_automation_mapping(&draft, &target, &clip, mapping)?;
            clip.bindings.push(AutomationBinding { target, mapping });
            draft.automation_clips.push(clip);
        }
        SequenceGuiEdit::MoveAutomationClip {
            id,
            start_seconds,
            row_target,
        } => {
            let clip = automation_clip_mut(&mut draft, id)?;
            clip.start = super::checked_gui_time(start_seconds.max(0.0))?;
            clip.row_target = layout_target_to_effect_target(&layout, row_target);
        }
        SequenceGuiEdit::SplitAutomationClip { id, time_seconds } => {
            let next_id = draft
                .automation_clips
                .iter()
                .map(|clip| clip.id.0)
                .max()
                .unwrap_or(0)
                + 1;
            let at = super::checked_gui_time(time_seconds)?;
            let index = draft
                .automation_clips
                .iter()
                .position(|clip| clip.id.0 == id)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Automation clip was not found.".to_string())
                })?;
            let right = draft.automation_clips[index]
                .split_off(at, AutomationClipId(next_id))
                .ok_or_else(|| {
                    GuiMutationError::Invalid(
                        "Split time must fall inside the automation clip.".to_string(),
                    )
                })?;
            draft.automation_clips.insert(index + 1, right);
        }
        SequenceGuiEdit::UpdateAutomationCurve { id, curve } => {
            let mut curve = curve_from_points(curve);
            curve.collapse_coincident_points();
            automation_clip_mut(&mut draft, id)?.curve = curve;
        }
        SequenceGuiEdit::UpdateAutomationParamMapping {
            clip_id,
            target,
            mapping,
        } => {
            let target = automation_target_from_gui(target)?;
            donder_language::validation::automation_target_type(
                &session.project,
                session.project.sequence(&sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?,
                &target,
            )
            .map_err(|error| GuiMutationError::Invalid(error.message))?;
            if !automation_clip_mut(&mut draft, clip_id)?
                .bindings
                .iter()
                .any(|binding| binding.target == target)
            {
                return Err(GuiMutationError::Invalid(
                    "Automation binding was not found.".to_string(),
                ));
            }
            // Every clip bound to a target shares its mapping.
            let mapping = automation_mapping_from_gui(mapping)?;
            for binding in draft
                .automation_clips
                .iter_mut()
                .flat_map(|clip| &mut clip.bindings)
                .filter(|binding| binding.target == target)
            {
                binding.mapping = mapping.clone();
            }
        }
        SequenceGuiEdit::DeleteAutomationClip { id } => {
            draft.automation_clips.retain(|clip| clip.id.0 != id);
        }
        SequenceGuiEdit::BindAutomationParam {
            clip_id,
            target,
            mapping,
        } => {
            let target = automation_target_from_gui(target)?;
            donder_language::validation::automation_target_type(
                &session.project,
                session.project.sequence(&sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?,
                &target,
            )
            .map_err(|error| GuiMutationError::Invalid(error.message))?;
            let clip = automation_clip_mut(&mut draft, clip_id)?.clone();
            let mapping = available_automation_mapping(
                &draft,
                &target,
                &clip,
                automation_mapping_from_gui(mapping)?,
            )?;
            automation_clip_mut(&mut draft, clip_id)?.bind(target, mapping);
        }
        SequenceGuiEdit::UnbindAutomationParam { clip_id, target } => {
            let target = automation_target_from_gui(target)?;
            let sequence = &mut draft;
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
                session.project.sequence(&sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?,
                &target,
            )
            .map_err(|error| GuiMutationError::Invalid(error.message))?;
            let clip = automation_clip_mut(&mut draft, clip_id)?.clone();
            let mapping = available_automation_mapping(
                &draft,
                &target,
                &clip,
                automation_mapping_from_gui(mapping)?,
            )?;
            let clip = automation_clip_mut(&mut draft, clip_id)?;
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
            let clip = automation_clip_mut(&mut draft, clip_id)?;
            if detached_index as usize >= clip.detached_bindings.len() {
                return Err(GuiMutationError::Invalid(
                    "Detached automation binding was not found.".to_string(),
                ));
            }
            clip.detached_bindings.remove(detached_index as usize);
        }
    }
    session
        .project
        .replace_sequence(&sequence_id, draft)
        .map_err(GuiMutationError::Invalid)
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
) -> Result<
    (
        DonderTime,
        DonderDuration,
        donder_language::layout::FixtureTarget,
    ),
    GuiMutationError,
> {
    match target {
        AutomationTarget::EffectParam { effect_id, .. } => {
            let effect = sequence
                .effects
                .iter()
                .find(|effect| &effect.id == effect_id)
                .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))?;
            Ok((
                effect.start.clone(),
                effect.duration.clone(),
                effect.target.clone(),
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
                super::checked_gui_time(0.0)?,
                sequence.duration.clone(),
                super::selection::target_for_lane(session, 0).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence has no target for automation.".into())
                })?,
            ))
        }
    }
}

/// Clips may share a target when they do not overlap; they share its mapping.
fn available_automation_mapping(
    sequence: &donder_language::sequence::Sequence,
    target: &AutomationTarget,
    clip: &AutomationClip,
    mapping: AutomationMapping,
) -> Result<AutomationMapping, GuiMutationError> {
    if sequence.automation_clips.iter().any(|other| {
        if other.id == clip.id {
            other
                .bindings
                .iter()
                .any(|binding| &binding.target == target)
        } else {
            other.overlaps(clip) && other.targets().any(|claimed| claimed == target)
        }
    }) {
        return Err(GuiMutationError::Invalid(
            "Param is already automated here.".to_string(),
        ));
    }
    Ok(sequence
        .automation_mapping(target)
        .cloned()
        .unwrap_or(mapping))
}

fn effect_ref_from_gui(
    session: &ProjectSession,
    reference: SequenceEffectReference,
) -> Result<EffectRef, GuiMutationError> {
    Ok(match reference {
        SequenceEffectReference::Custom {
            module_id,
            path,
            effect_name,
        } => {
            let identity = source_identity_from_gui(&module_id, &path, &effect_name)?;
            if !session.source.is_project_owned(identity.document_id()) {
                return Err(GuiMutationError::Invalid(
                    "Effect source module was not found.".to_string(),
                ));
            }
            EffectRef::Custom(EffectDefinitionId(identity))
        }
    })
}

use donder_language::effect::{
    EffectDefinitionId, EffectInst, EffectInstId, EffectParamValue, EffectRef,
};
use donder_language::operator::{GraphOperatorNode, OperatorRef, validate_composition_graph};
use donder_language::sequence::{
    AutomationBinding, AutomationClip, AutomationClipId, AutomationDetachmentReason,
    AutomationMapping, AutomationTarget, CompositionGraphNode, CompositionGraphNodeId,
    CompositionGraphNodeKind, GraphNodePosition, MarkCollection, MarkCollectionKey,
    SequenceAudio as DomainSequenceAudio, SequenceId, SequenceLayerId,
};
use donder_language::values::{DonderDuration, DonderTime};
use donder_project_io::{ProjectSession, SourceObjectKind, ensure_document_can_reference_source};
use indexmap::IndexMap;

use super::model::{
    automation_binding_value_at, automation_clip_mut, automation_mapping_from_gui,
    composition_graph_node_mut, create_sequence_layer, curve_from_points, default_automation_curve,
    effect_mut, effect_param_value_from_gui, effect_scope, graph_operator_from_gui, identifier,
    layout_target_to_effect_target, mark_collection_mut, next_composition_node_id, parse_color,
    parse_graph_node_id, register_sequence_audio_asset, source_identity_from_gui,
};
use super::selection::{mark_param_names, required_operator_param_value};
use super::{GuiMutationError, ResolvedGuiObject};
use crate::dto::{SequenceAutomationTarget, SequenceEffectReference, SequenceGuiEdit};
