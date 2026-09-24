use super::mapping::{MappingReader, parse_mapping};
use camino::{Utf8Path, Utf8PathBuf};
use donder_language::fixture::*;
use donder_language::identity::DocumentId;
use donder_language::layout::*;
use donder_language::patch::*;
pub(super) struct DomainResolver<'a> {
    pub(super) loader: &'a mut Loader,
    pub(super) project: &'a mut DonderProject,
}

impl DomainResolver<'_> {
    pub(super) fn resolve_setup(&mut self, id: &SetupId) -> Result<(), LoadProjectError> {
        if self.project.setups.contains_key(id) {
            return Ok(());
        }
        let (document_id, _, value) = self
            .loader
            .object_value(&ResolvedObject::Setup(id.clone()))?;
        let document_path = document_id.path().to_path_buf();

        parse_mapping(&document_path, &value, "setup", |fields| {
            fields.string("type")?;
            let layout_ref = fields.string("layout")?;
            let patch_ref = fields.string("patch")?;
            let layout = match self.loader.resolve_reference(&document_id, layout_ref)? {
                ResolvedObject::Layout(id) => id,
                _ => return Err(invalid(&document_path, "Expected a layout reference.")),
            };
            let patch = match self.loader.resolve_reference(&document_id, patch_ref)? {
                ResolvedObject::Patch(patch) => patch,
                _ => {
                    return Err(LoadProjectError::InvalidReference {
                        path: document_path.clone(),
                        range: source_range_for_scalar(&document_path, patch_ref),
                        reference: patch_ref.to_string(),
                    });
                }
            };
            let controllers = fields
                .strings("controllers")?
                .iter()
                .map(
                    |reference| match self.loader.resolve_reference(&document_id, reference)? {
                        ResolvedObject::Controller(controller) => Ok(controller),
                        _ => Err(LoadProjectError::InvalidReference {
                            path: document_path.clone(),
                            range: source_range_for_scalar(&document_path, reference),
                            reference: reference.clone(),
                        }),
                    },
                )
                .collect::<Result<Vec<_>, _>>()?;
            self.project.setups.insert(
                id.clone(),
                Setup {
                    id: id.clone(),
                    layout: layout.clone(),
                    patch: patch.clone(),
                    controllers: controllers.clone(),
                },
            );
            self.resolve_layout(&layout)?;
            self.resolve_patch(&patch)?;
            for controller in controllers {
                self.resolve_controller(&controller)?;
            }
            Ok(())
        })
    }

    pub(super) fn resolve_controller(&mut self, id: &ControllerId) -> Result<(), LoadProjectError> {
        if self.project.controllers.contains_key(id) {
            return Ok(());
        }
        let (document_id, _, value) = self
            .loader
            .object_value(&ResolvedObject::Controller(id.clone()))?;
        let path = document_id.path().to_path_buf();

        parse_mapping(&path, &value, "controller", |fields| {
            fields.string("type")?;
            let protocol_value = fields.required("protocol")?;
            let protocol = parse_mapping(
                &path,
                protocol_value,
                "controller protocol",
                |protocol_fields| {
                    Ok(match protocol_fields.string("type")? {
                        "e131" => {
                            let mode = match protocol_fields.string("mode")? {
                                "multicast" => E131Mode::Multicast,
                                "unicast" => E131Mode::Unicast {
                                    destination: protocol_fields
                                        .string("destination")?
                                        .parse()
                                        .map_err(|_| {
                                            invalid(&path, "invalid E1.31 destination address")
                                        })?,
                                },
                                other => {
                                    return Err(invalid(
                                        &path,
                                        &format!("invalid E1.31 mode `{other}`"),
                                    ));
                                }
                            };
                            ControllerProtocol::E131(E131Config {
                                source_name: protocol_fields.string("source_name")?.to_string(),
                                bind_address: protocol_fields
                                    .string("bind_address")?
                                    .parse()
                                    .map_err(|_| invalid(&path, "invalid E1.31 bind address"))?,
                                priority: u8::try_from(protocol_fields.u32("priority")?)
                                    .map_err(|_| invalid(&path, "E1.31 priority must be a u8"))?,
                                mode,
                            })
                        }
                        "artnet" => ControllerProtocol::ArtNet(ArtNetConfig {
                            bind_address: protocol_fields
                                .string("bind_address")?
                                .parse()
                                .map_err(|_| invalid(&path, "invalid Art-Net bind socket"))?,
                            destination: protocol_fields.string("destination")?.parse().map_err(
                                |_| invalid(&path, "invalid Art-Net destination socket"),
                            )?,
                            mode: match protocol_fields.string("mode")? {
                                "unicast" => ArtNetMode::Unicast,
                                "broadcast" => ArtNetMode::Broadcast,
                                other => {
                                    return Err(invalid(
                                        &path,
                                        &format!("invalid Art-Net mode `{other}`"),
                                    ));
                                }
                            },
                        }),
                        other => {
                            return Err(invalid(
                                &path,
                                &format!("unsupported controller protocol `{other}`"),
                            ));
                        }
                    })
                },
            )?;
            let ports = fields
                .sequence("ports")?
                .iter()
                .map(|port| {
                    parse_mapping(&path, port, "controller port", |port_fields| {
                        let id = ControllerPortId(port_fields.u32("id")?);
                        let slot_count = u16::try_from(port_fields.u32("slot_count")?)
                            .map_err(|_| invalid(&path, "controller slot count must be a u16"))?;
                        let address = match &protocol {
                            ControllerProtocol::E131(_) => ControllerPortAddress::E131Universe(
                                u16::try_from(port_fields.u32("universe")?)
                                    .map_err(|_| invalid(&path, "E1.31 universe must be a u16"))?,
                            ),
                            ControllerProtocol::ArtNet(_) => ControllerPortAddress::ArtNetPort(
                                u16::try_from(port_fields.u32("port_address")?).map_err(|_| {
                                    invalid(&path, "Art-Net port address must be a u16")
                                })?,
                            ),
                        };
                        Ok(ControllerPort {
                            id,
                            address,
                            slot_count,
                        })
                    })
                })
                .collect::<Result<Vec<_>, LoadProjectError>>()?;
            let controller = Controller { protocol, ports };
            controller
                .validate()
                .map_err(|error| invalid(&path, &format!("invalid controller: {error:?}")))?;
            self.project.controllers.insert(id.clone(), controller);
            Ok(())
        })
    }

    pub(super) fn resolve_fixture(
        &mut self,
        id: &FixtureDefinitionId,
    ) -> Result<(), LoadProjectError> {
        if self
            .project
            .definitions
            .fixtures
            .definitions
            .contains_key(id)
        {
            return Ok(());
        }
        let (document, _, value) = self
            .loader
            .object_value(&ResolvedObject::FixtureDefinition(id.clone()))?;

        parse_mapping(document.path(), &value, "fixture definition", |fields| {
            fields.string("type")?;
            let pixels = fields
                .sequence("pixels")?
                .iter()
                .map(|value| Self::parse_pixel(&document, value))
                .collect::<Result<Vec<_>, _>>()?;
            self.project
                .definitions
                .fixtures
                .definitions
                .insert(id.clone(), FixtureDefinition { pixels });
            Ok(())
        })
    }

    fn fixture_reference(
        &self,
        document: &DocumentId,

        fields: &MappingReader<'_>,
        key: &str,
    ) -> Result<FixtureDefinitionId, LoadProjectError> {
        let reference = fields.string(key)?;
        match self.loader.resolve_reference(document, reference)? {
            ResolvedObject::FixtureDefinition(id) => Ok(id),
            _ => Err(invalid(
                document.path(),
                "Expected a fixture definition reference.",
            )),
        }
    }

    fn parse_pixel(document: &DocumentId, value: &Value) -> Result<Pixel, LoadProjectError> {
        let path = document.path();

        parse_mapping(path, value, "pixel", |fields| {
            let diameter = fields
                .required("diameter")?
                .as_f64()
                .ok_or_else(|| invalid(path, "Pixel diameter must be a number."))?;
            if !diameter.is_finite() || diameter < 0.000001 || diameter > 100.0 {
                return Err(invalid(
                    path,
                    "Pixel diameter must be between 0.000001 and 100 meters.",
                ));
            }
            Ok(Pixel {
                id: PixelId(fields.u32("id")?),
                position: fields
                    .optional("position")
                    .map(|point| parse_point3(path, point))
                    .transpose()?
                    .unwrap_or_default(),
                diameter: donder_language::values::DistanceSpan {
                    micrometers: (diameter * 1_000_000.0).round() as u32,
                },
            })
        })
    }

    pub(super) fn resolve_layout(&mut self, id: &LayoutId) -> Result<(), LoadProjectError> {
        if self.project.layouts.contains_key(id) {
            return Ok(());
        }
        let (document, _, value) = self
            .loader
            .object_value(&ResolvedObject::Layout(id.clone()))?;

        parse_mapping(document.path(), &value, "layout", |fields| {
            fields.string("type")?;
            let fixtures = fields
                .sequence("fixtures")?
                .iter()
                .map(|value| self.parse_layout_fixture(&document, value))
                .collect::<Result<_, _>>()?;
            self.project.layouts.insert(
                id.clone(),
                Layout {
                    id: id.clone(),
                    fixtures,
                },
            );
            Ok(())
        })
    }

    fn parse_layout_fixture(
        &self,
        document: &DocumentId,
        value: &Value,
    ) -> Result<LayoutFixture, LoadProjectError> {
        let path = document.path();

        parse_mapping(path, value, "layout fixture", |fields| {
            let kind = match fields.string("type")? {
                "fixture" => LayoutFixtureKind::Fixture {
                    definition: self.fixture_reference(document, fields, "definition")?,
                    transform: parse_fixture_transform(path, fields.optional("transform"))?,
                },
                "group" => LayoutFixtureKind::Group {
                    children: fields
                        .sequence("children")?
                        .iter()
                        .map(|child| self.parse_layout_fixture(document, child))
                        .collect::<Result<_, _>>()?,
                },
                _ => return Err(invalid(path, "Expected fixture or group.")),
            };
            Ok(LayoutFixture {
                id: FixtureInstanceId(fields.u32("id")?),
                name: fields.string("name")?.to_owned(),
                kind,
            })
        })
    }

    pub(super) fn resolve_patch(&mut self, id: &PatchId) -> Result<(), LoadProjectError> {
        if self.project.patches.contains_key(id) {
            return Ok(());
        }
        let (document, _, value) = self
            .loader
            .object_value(&ResolvedObject::Patch(id.clone()))?;

        parse_mapping(document.path(), &value, "patch", |fields| {
            fields.string("type")?;
            let routes = fields
                .sequence("routes")?
                .iter()
                .map(|value| self.parse_pixel_route(&document, value))
                .collect::<Result<_, _>>()?;
            self.project.patches.insert(
                id.clone(),
                Patch {
                    id: id.clone(),
                    routes,
                },
            );
            Ok(())
        })
    }

    fn parse_fixture_target(
        &self,
        document: &DocumentId,
        value: &Value,
    ) -> Result<FixtureTarget, LoadProjectError> {
        let path = document.path();

        parse_mapping(path, value, "fixture target", |fields| {
            let layout = match self
                .loader
                .resolve_reference(document, fields.string("layout")?)?
            {
                ResolvedObject::Layout(id) => id,
                _ => return Err(invalid(path, "Expected a layout reference.")),
            };
            Ok(FixtureTarget {
                layout,
                fixture: FixtureInstanceId(fields.u32("fixture")?),
            })
        })
    }

    fn parse_pixel_route(
        &self,
        document: &DocumentId,
        value: &Value,
    ) -> Result<PixelRoute, LoadProjectError> {
        let path = document.path();

        parse_mapping(path, value, "LED route", |fields| {
            let controller = match self
                .loader
                .resolve_reference(document, fields.string("controller")?)?
            {
                ResolvedObject::Controller(id) => id,
                _ => return Err(invalid(path, "Expected a controller reference.")),
            };
            let pixels = fields
                .optional("pixels")
                .map(|span| {
                    parse_mapping(path, span, "output pixel span", |span_fields| {
                        Ok::<_, LoadProjectError>(PixelSpan {
                            start: span_fields.u32("start")?,
                            count: span_fields.u32("count")?,
                        })
                    })
                })
                .transpose()?;
            let encoding = fields.required("encoding")?;
            let encoding = parse_mapping(path, encoding, "pixel encoding", |encoding_fields| {
                let order = encoding_fields
                    .sequence("order")?
                    .iter()
                    .map(|channel| {
                        channel
                            .as_u64()
                            .and_then(|channel| u8::try_from(channel).ok())
                            .ok_or_else(|| {
                                invalid(path, "Channel order must contain byte indices.")
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(match encoding_fields.string("type")? {
                    "rgb" => PixelEncoding::Rgb {
                        order: order
                            .try_into()
                            .map_err(|_| invalid(path, "RGB needs three channel indices."))?,
                    },
                    "rgbw" => PixelEncoding::Rgbw {
                        order: order
                            .try_into()
                            .map_err(|_| invalid(path, "RGBW needs four channel indices."))?,
                    },
                    _ => return Err(invalid(path, "Expected rgb or rgbw encoding.")),
                })
            })?;
            Ok(PixelRoute {
                id: PixelRouteId(fields.u32("id")?),
                target: self.parse_fixture_target(document, fields.required("target")?)?,
                pixels,
                controller,
                port: ControllerPortId(fields.u32("port")?),
                start_slot: fields
                    .u32("start_slot")?
                    .try_into()
                    .map_err(|_| invalid(path, "Start slot is out of range."))?,
                encoding,
                gamma: fields.f32("gamma")?,
                brightness: fields.f32("brightness")?,
            })
        })
    }

    pub(super) fn resolve_sequence(&mut self, id: &SequenceId) -> Result<(), LoadProjectError> {
        if self.project.sequences.contains_key(id) {
            return Ok(());
        }
        let (document_id, _, value) = self
            .loader
            .object_value(&ResolvedObject::Sequence(id.clone()))?;
        let document_path = document_id.path().to_path_buf();

        parse_mapping(&document_path, &value, "sequence", |fields| {
            fields.string("type")?;
            let duration = parse_duration(fields.string("duration")?).map_err(|error| {
                with_yaml_location(
                    error,
                    &document_path,
                    source_range_for_field_value(&document_path, &value, "duration"),
                )
            })?;
            let audio = self.parse_audio(&document_id, fields)?;
            let mark_collections = fields
                .optional_sequence("mark_collections")?
                .into_iter()
                .flatten()
                .map(|collection| parse_mark_collection(&document_path, collection))
                .collect::<Result<Vec<_>, _>>()?;
            let layers = fields
                .sequence("layers")?
                .iter()
                .map(|layer| parse_sequence_layer(&document_path, layer))
                .collect::<Result<Vec<_>, _>>()?;
            let effects = fields
                .sequence("effects")?
                .iter()
                .map(|effect| self.parse_sequence_effect(&document_id, effect))
                .collect::<Result<Vec<_>, _>>()?;
            let composition_graph =
                self.parse_composition_graph(&document_id, fields.required("composition_graph")?)?;
            let automation_clips = fields
                .optional_sequence("automation_clips")?
                .into_iter()
                .flatten()
                .map(|clip| self.parse_automation_clip(&document_path, clip))
                .collect::<Result<Vec<_>, _>>()?;
            let mut automation_targets = IndexSet::new();
            for target in automation_clips.iter().flat_map(|clip| {
                clip.bindings
                    .iter()
                    .map(|binding| &binding.target)
                    .chain(clip.detached_bindings.iter().map(|binding| &binding.target))
            }) {
                if !automation_targets.insert(target.clone()) {
                    return Err(LoadProjectError::InvalidDocument {
                        path: document_path.clone(),
                        range: source_range_for_field_value(
                            &document_path,
                            &value,
                            "automation_clips",
                        ),
                        message: "sequence has duplicate automation targets".to_string(),
                    });
                }
            }
            self.project.sequences.insert(
                id.clone(),
                Sequence {
                    id: id.clone(),
                    duration,
                    frame_rate: fields.u32("frame_rate")?,
                    audio,
                    mark_collections,
                    layers,
                    effects,
                    composition_graph,
                    automation_clips,
                },
            );
            Ok(())
        })
    }

    pub(super) fn parse_audio(
        &mut self,
        document_id: &donder_language::identity::DocumentId,

        fields: &MappingReader<'_>,
    ) -> Result<SequenceAudio, LoadProjectError> {
        let path = document_id.path();
        let Some(audio) = fields.optional("audio") else {
            return Ok(SequenceAudio::None);
        };
        if matches!(audio, Value::Null) {
            return Ok(SequenceAudio::None);
        }
        let Some(audio_path) = audio.as_str() else {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: "audio must be null or a path string".to_string(),
            });
        };
        let module = self
            .loader
            .source_graph
            .module(document_id.module_id())
            .map_err(|error| LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: error.to_string(),
            })?;
        if !module.manifest.assets.contains_key(audio_path) {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: format!(
                    "audio asset `{audio_path}` is not declared in donder-package.json"
                ),
            });
        }
        let unresolved = module.root.join(audio_path);
        let absolute = unresolved
            .canonicalize_utf8()
            .map_err(|source| LoadProjectError::Io {
                path: unresolved,
                source,
            })?;
        if !absolute.is_file() || !absolute.starts_with(&module.root) {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: format!("audio asset does not exist inside its module: {audio_path}"),
            });
        }
        let relative = Utf8PathBuf::from(audio_path);
        if let Some(existing) = self.loader.referenced_assets.iter_mut().find(|asset| {
            asset.module_id == document_id.module_id() && asset.relative_path == relative
        }) {
            existing.referenced_by.insert(document_id.clone());
            return Ok(SequenceAudio::Asset(existing.id.clone()));
        }
        let id = AssetId(self.loader.next_asset_id);
        self.loader.next_asset_id += 1;
        self.loader.referenced_assets.push(ReferencedAsset {
            id: id.clone(),
            module_id: document_id.module_id(),
            relative_path: relative,
            absolute_path: absolute,
            referenced_by: std::collections::BTreeSet::from([document_id.clone()]),
        });
        Ok(SequenceAudio::Asset(id))
    }

    pub(super) fn parse_sequence_effect(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
    ) -> Result<EffectInst, LoadProjectError> {
        let path = document_id.path();

        parse_mapping(path, value, "effect instance", |fields| {
            let definition = self.parse_effect_definition(document_id, value, fields)?;
            let param_overrides = self.parse_param_overrides(document_id, fields)?;
            Ok(EffectInst {
                id: EffectInstId(fields.u32("id")?),
                layer_id: SequenceLayerId(fields.u32("layer_id")?),
                start: parse_duration_as_time(fields.string("start")?).map_err(|error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "start"),
                    )
                })?,
                duration: parse_duration(fields.string("duration")?).map_err(|error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "duration"),
                    )
                })?,
                target: self.parse_fixture_target(document_id, fields.required("target")?)?,
                scope: parse_effect_scope(path, value, fields)?,
                definition,
                param_overrides,
            })
        })
    }

    pub(super) fn parse_composition_graph(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
    ) -> Result<SequenceCompositionGraph, LoadProjectError> {
        let path = document_id.path();

        parse_mapping(path, value, "composition graph", |fields| {
            let graph = SequenceCompositionGraph {
                nodes: fields
                    .sequence("nodes")?
                    .iter()
                    .map(|node| self.parse_composition_graph_node(document_id, node))
                    .collect::<Result<Vec<_>, _>>()?,
                edges: fields
                    .sequence("edges")?
                    .iter()
                    .map(|edge| parse_graph_edge(path, edge))
                    .collect::<Result<Vec<_>, _>>()?,
            };
            validate_composition_graph(&graph, &self.project.definitions.operators).map_err(
                |error| LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: None,
                    message: error.message,
                },
            )?;
            Ok(graph)
        })
    }

    pub(super) fn parse_composition_graph_node(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
    ) -> Result<CompositionGraphNode, LoadProjectError> {
        let path = document_id.path();

        parse_mapping(path, value, "graph node", |fields| {
            let kind = match fields.string("type")? {
                "layer" => CompositionGraphNodeKind::Layer {
                    layer_id: SequenceLayerId(fields.u32("layer_id")?),
                },
                "operator" => CompositionGraphNodeKind::Operator(GraphOperatorNode {
                    operator: self.parse_graph_operator_ref(document_id, value, fields)?,
                    params: self.parse_param_overrides(document_id, fields)?,
                }),
                "output" => CompositionGraphNodeKind::Output,
                other => {
                    return Err(LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_field_value(path, value, "type"),
                        message: format!("unsupported composition graph node type `{other}`"),
                    });
                }
            };
            Ok(CompositionGraphNode {
                id: CompositionGraphNodeId(fields.u32("id")?),
                position: parse_graph_position(path, fields.required("position")?)?,
                kind,
            })
        })
    }

    fn parse_effect_definition(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
        fields: &MappingReader<'_>,
    ) -> Result<EffectRef, LoadProjectError> {
        let path = document_id.path();
        let effect_ref = fields.string("effect")?;
        let reference = donder_language::imports::SourceReference::parse(effect_ref)
            .ok()
            .and_then(|reference| {
                crate::imports::lookup_effect_reference(
                    &self.loader.visible_objects,
                    document_id,
                    &reference,
                )
            })
            .ok_or_else(|| LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, value, "effect"),
                reference: effect_ref.to_string(),
            })?;
        if let EffectRef::Custom(definition) = &reference {
            self.resolve_effect_definition(definition)?;
        }
        Ok(reference)
    }

    fn parse_param_overrides(
        &mut self,
        document_id: &donder_language::identity::DocumentId,

        fields: &MappingReader<'_>,
    ) -> Result<IndexMap<Identifier, EffectParamValue>, LoadProjectError> {
        let path = document_id.path();
        Ok(fields
            .dictionary("params", |key, value| {
                let identifier = Identifier::new(key.to_string()).map_err(|_| {
                    LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: format!("invalid parameter name `{key}`"),
                    }
                })?;
                Ok((identifier, self.parse_effect_param(document_id, value)?))
            })?
            .into_iter()
            .collect())
    }

    pub(super) fn parse_graph_operator_ref(
        &self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
        fields: &MappingReader<'_>,
    ) -> Result<OperatorRef, LoadProjectError> {
        let path = document_id.path();
        let name = fields.string("operator")?;
        if let Some(builtin) = BuiltinOperator::from_source_name(name) {
            return Ok(OperatorRef::Builtin(builtin));
        }
        match self.loader.resolve_reference(document_id, name)? {
            ResolvedObject::OperatorDefinition(id) => Ok(OperatorRef::Custom(id)),
            _ => Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, value, "operator"),
                reference: name.to_string(),
            }),
        }
    }

    pub(super) fn resolve_effect_definition(
        &mut self,
        id: &EffectDefinitionId,
    ) -> Result<(), LoadProjectError> {
        if !self
            .project
            .definitions
            .effects
            .definitions
            .contains_key(id)
        {
            return Err(LoadProjectError::InvalidReference {
                path: id.0.document().to_path_buf(),
                range: None,
                reference: id.0.object().to_string(),
            });
        }
        Ok(())
    }

    pub(super) fn parse_effect_param(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
    ) -> Result<EffectParamValue, LoadProjectError> {
        let path = document_id.path();

        parse_mapping(path, value, "effect parameter value", |fields| {
            self.parse_effect_param_fields(document_id, value, fields)
        })
    }

    fn parse_effect_param_fields(
        &mut self,
        document_id: &DocumentId,
        value: &Value,
        fields: &MappingReader<'_>,
    ) -> Result<EffectParamValue, LoadProjectError> {
        let path = document_id.path();
        let kind = fields.string("type")?;
        match kind {
            "integer" => Ok(EffectParamValue::Int(fields.i32("value")?)),
            "float" => Ok(EffectParamValue::Float(fields.f32("value")?)),
            "bool" => Ok(EffectParamValue::Bool(fields.bool("value")?)),
            "color" => Ok(EffectParamValue::Color(
                parse_color(fields.string("value")?).map_err(|error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "value"),
                    )
                })?,
            )),
            "enum" => Ok(EffectParamValue::Enum(
                Identifier::new(fields.string("value")?.to_string()).map_err(|_| {
                    LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "invalid enum value".to_string(),
                    }
                })?,
            )),
            "marks" => Ok(EffectParamValue::Marks(MarkCollectionKey {
                name: fields.string("key")?.to_string(),
            })),
            "curve" => Ok(EffectParamValue::Curve(
                self.parse_curve_source(document_id, fields.required("curve")?)?,
            )),
            "gradient" => Ok(EffectParamValue::Gradient(
                self.parse_gradient_source(document_id, fields.required("gradient")?)?,
            )),
            "array" => {
                let values = fields
                    .sequence("values")?
                    .iter()
                    .map(|item| self.parse_array_item(document_id, item))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(EffectParamValue::Array(values))
            }
            other => Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: format!("unsupported effect param type `{other}`"),
            }),
        }
    }

    pub(super) fn parse_array_item(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
    ) -> Result<EffectParamValue, LoadProjectError> {
        let path = document_id.path();
        parse_mapping(path, value, "array item", |fields| {
            if fields.optional("type").is_some() {
                return self.parse_effect_param_fields(document_id, value, fields);
            }
            if let Some(curve) = fields.optional("curve") {
                return Ok(EffectParamValue::Curve(
                    self.parse_curve_source(document_id, curve)?,
                ));
            }
            let gradient = fields.required("gradient")?;
            Ok(EffectParamValue::Gradient(
                self.parse_gradient_source(document_id, gradient)?,
            ))
        })
    }

    pub(super) fn parse_curve_source(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
    ) -> Result<CurveSource, LoadProjectError> {
        let path = document_id.path();
        if let Some(reference) = value.as_str() {
            let id = match self.loader.resolve_reference(document_id, reference)? {
                ResolvedObject::Curve(curve) => curve,
                _ => {
                    return Err(LoadProjectError::InvalidReference {
                        path: path.to_path_buf(),
                        range: source_range_for_scalar(path, reference),
                        reference: reference.to_string(),
                    });
                }
            };
            self.resolve_curve(path, &id)?;
            return Ok(CurveSource::Reference(id));
        }
        parse_mapping(path, value, "curve source", |fields| {
            if let Some(curve_value) = fields.optional("curve") {
                return self.parse_curve_source(document_id, curve_value);
            }
            Ok(CurveSource::Inline(super::parse::parse_curve_fields(
                path, value, fields,
            )?))
        })
    }

    pub(super) fn resolve_curve(
        &mut self,
        path: &Utf8Path,
        id: &CurveId,
    ) -> Result<(), LoadProjectError> {
        if !self.project.definitions.curves.definitions.contains_key(id) {
            return Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: None,
                reference: id.0.object().to_string(),
            });
        }
        Ok(())
    }

    pub(super) fn parse_gradient_source(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
    ) -> Result<GradientSource, LoadProjectError> {
        let path = document_id.path();
        if let Some(reference) = value.as_str() {
            let id = match self.loader.resolve_reference(document_id, reference)? {
                ResolvedObject::Gradient(gradient) => gradient,
                _ => {
                    return Err(LoadProjectError::InvalidReference {
                        path: path.to_path_buf(),
                        range: source_range_for_scalar(path, reference),
                        reference: reference.to_string(),
                    });
                }
            };
            if !self
                .project
                .definitions
                .gradients
                .definitions
                .contains_key(&id)
            {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    range: None,
                    reference: id.0.object().to_string(),
                });
            }
            return Ok(GradientSource::Reference(id));
        }
        parse_mapping(path, value, "gradient source", |fields| {
            if let Some(gradient) = fields.optional("gradient") {
                return self.parse_gradient_source(document_id, gradient);
            }
            Ok(GradientSource::Inline(super::parse::parse_gradient_fields(
                path, fields,
            )?))
        })
    }

    pub(super) fn parse_automation_clip(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<AutomationClip, LoadProjectError> {
        parse_mapping(path, value, "automation clip", |fields| {
            let bindings = fields
                .sequence("bindings")?
                .iter()
                .map(|binding| parse_automation_binding(path, binding))
                .collect::<Result<Vec<_>, _>>()?;
            let detached_bindings = fields
                .optional_sequence("detached_bindings")?
                .into_iter()
                .flatten()
                .map(|binding| parse_detached_automation_binding(path, binding))
                .collect::<Result<Vec<_>, _>>()?;
            let mut seen = IndexSet::new();
            for target in bindings
                .iter()
                .map(|binding| &binding.target)
                .chain(detached_bindings.iter().map(|binding| &binding.target))
            {
                if !seen.insert(target.clone()) {
                    return Err(LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_field_value(path, value, "bindings"),
                        message: "automation clip has duplicate bindings for a parameter"
                            .to_string(),
                    });
                }
            }
            Ok(AutomationClip {
                id: AutomationClipId(fields.u32("id")?),
                start: parse_duration_as_time(fields.string("start")?).map_err(|error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "start"),
                    )
                })?,
                duration: parse_duration(fields.string("duration")?).map_err(|error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "duration"),
                    )
                })?,
                anchor_lane_index: fields.u32("anchor_lane_index")?,
                lane_index: fields.u32("lane_index")?,
                curve: parse_automation_curve(path, fields.required("curve")?)?,
                bindings,
                detached_bindings,
            })
        })
    }
}

fn invalid(path: &Utf8Path, message: &str) -> LoadProjectError {
    LoadProjectError::InvalidDocument {
        path: path.to_path_buf(),
        range: None,
        message: message.to_string(),
    }
}

use donder_language::controller::{
    ArtNetConfig, ArtNetMode, Controller, ControllerId, ControllerPort, ControllerPortAddress,
    ControllerPortId, ControllerProtocol, E131Config, E131Mode,
};
use donder_language::dsl::Identifier;
use donder_language::effect::{
    CurveId, CurveSource, EffectDefinitionId, EffectInst, EffectInstId, EffectParamValue,
    EffectRef, GradientSource,
};
use donder_language::model::DonderProject;
use donder_language::operator::{
    BuiltinOperator, GraphOperatorNode, OperatorRef, validate_composition_graph,
};
use donder_language::sequence::{
    AssetId, AutomationClip, AutomationClipId, CompositionGraphNode, CompositionGraphNodeId,
    CompositionGraphNodeKind, MarkCollectionKey, Sequence, SequenceAudio, SequenceCompositionGraph,
    SequenceId, SequenceLayerId,
};
use donder_language::setup::{Setup, SetupId};
use indexmap::{IndexMap, IndexSet};
use yaml_serde::Value;

use super::Loader;
use super::parse::{
    ResolvedObject, parse_automation_binding, parse_automation_curve, parse_color,
    parse_detached_automation_binding, parse_duration, parse_duration_as_time, parse_effect_scope,
    parse_graph_edge, parse_graph_position, parse_mark_collection, parse_point3, parse_rotation3,
    parse_scale3, parse_sequence_layer,
};
use crate::LoadProjectError;
use crate::diagnostics::{
    source_range_for_field_value, source_range_for_scalar, with_yaml_location,
};
use crate::source::ReferencedAsset;

fn parse_fixture_transform(
    path: &Utf8Path,
    value: Option<&Value>,
) -> Result<FixtureTransform, LoadProjectError> {
    let Some(value) = value else {
        return Ok(FixtureTransform::default());
    };
    parse_mapping(path, value, "fixture transform", |fields| {
        Ok(FixtureTransform {
            position: fields
                .optional("position")
                .map(|point| parse_point3(path, point))
                .transpose()?
                .unwrap_or_default(),
            rotation: fields
                .optional("rotation")
                .map(|rotation| parse_rotation3(path, rotation))
                .transpose()?
                .unwrap_or_default(),
            scale: fields
                .optional("scale")
                .map(|scale| parse_scale3(path, scale))
                .transpose()?
                .unwrap_or_default(),
        })
    })
}
