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
        require_allowed_mapping_keys(
            &document_path,
            &value,
            &["type", "layout", "patch", "controllers"],
            "setup",
        )?;
        let layout_ref = string_field(&document_path, &value, "layout")?;
        let patch_ref = string_field(&document_path, &value, "patch")?;
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
        let controllers = sequence_field(&document_path, &value, "controllers")?
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
    }

    pub(super) fn resolve_controller(&mut self, id: &ControllerId) -> Result<(), LoadProjectError> {
        if self.project.controllers.contains_key(id) {
            return Ok(());
        }
        let (document_id, _, value) = self
            .loader
            .object_value(&ResolvedObject::Controller(id.clone()))?;
        let path = document_id.path().to_path_buf();
        let protocol_value = required_field(&path, &value, "protocol")?;
        require_allowed_mapping_keys(&path, &value, &["type", "protocol", "ports"], "controller")?;
        let protocol = match string_field(&path, protocol_value, "type")? {
            "e131" => {
                let mode = match string_field(&path, protocol_value, "mode")? {
                    "multicast" => {
                        require_allowed_mapping_keys(
                            &path,
                            protocol_value,
                            &["type", "source_name", "bind_address", "priority", "mode"],
                            "multicast E1.31 protocol",
                        )?;
                        E131Mode::Multicast
                    }
                    "unicast" => {
                        require_allowed_mapping_keys(
                            &path,
                            protocol_value,
                            &[
                                "type",
                                "source_name",
                                "bind_address",
                                "priority",
                                "mode",
                                "destination",
                            ],
                            "unicast E1.31 protocol",
                        )?;
                        E131Mode::Unicast {
                            destination: string_field(&path, protocol_value, "destination")?
                                .parse()
                                .map_err(|_| invalid(&path, "invalid E1.31 destination address"))?,
                        }
                    }
                    other => return Err(invalid(&path, &format!("invalid E1.31 mode `{other}`"))),
                };
                ControllerProtocol::E131(E131Config {
                    source_name: string_field(&path, protocol_value, "source_name")?.to_string(),
                    bind_address: string_field(&path, protocol_value, "bind_address")?
                        .parse()
                        .map_err(|_| invalid(&path, "invalid E1.31 bind address"))?,
                    priority: u8::try_from(u32_field(&path, protocol_value, "priority")?)
                        .map_err(|_| invalid(&path, "E1.31 priority must be a u8"))?,
                    mode,
                })
            }
            "artnet" => {
                require_allowed_mapping_keys(
                    &path,
                    protocol_value,
                    &["type", "bind_address", "destination", "mode"],
                    "Art-Net protocol",
                )?;
                ControllerProtocol::ArtNet(ArtNetConfig {
                    bind_address: string_field(&path, protocol_value, "bind_address")?
                        .parse()
                        .map_err(|_| invalid(&path, "invalid Art-Net bind socket"))?,
                    destination: string_field(&path, protocol_value, "destination")?
                        .parse()
                        .map_err(|_| invalid(&path, "invalid Art-Net destination socket"))?,
                    mode: match string_field(&path, protocol_value, "mode")? {
                        "unicast" => ArtNetMode::Unicast,
                        "broadcast" => ArtNetMode::Broadcast,
                        other => {
                            return Err(invalid(&path, &format!("invalid Art-Net mode `{other}`")));
                        }
                    },
                })
            }
            other => {
                return Err(invalid(
                    &path,
                    &format!("unsupported controller protocol `{other}`"),
                ));
            }
        };
        let ports = sequence_values(&path, &value, "ports")?
            .iter()
            .map(|port| {
                let fields: &[&str] = match &protocol {
                    ControllerProtocol::E131(_) => &["id", "slot_count", "universe"],
                    ControllerProtocol::ArtNet(_) => &["id", "slot_count", "port_address"],
                };
                require_allowed_mapping_keys(&path, port, fields, "controller port")?;
                let id = ControllerPortId(u32_field(&path, port, "id")?);
                let slot_count = u16::try_from(u32_field(&path, port, "slot_count")?)
                    .map_err(|_| invalid(&path, "controller slot count must be a u16"))?;
                let address = match &protocol {
                    ControllerProtocol::E131(_) => ControllerPortAddress::E131Universe(
                        u16::try_from(u32_field(&path, port, "universe")?)
                            .map_err(|_| invalid(&path, "E1.31 universe must be a u16"))?,
                    ),
                    ControllerProtocol::ArtNet(_) => ControllerPortAddress::ArtNetPort(
                        u16::try_from(u32_field(&path, port, "port_address")?)
                            .map_err(|_| invalid(&path, "Art-Net port address must be a u16"))?,
                    ),
                };
                Ok(ControllerPort {
                    id,
                    address,
                    slot_count,
                })
            })
            .collect::<Result<Vec<_>, LoadProjectError>>()?;
        let controller = Controller { protocol, ports };
        controller
            .validate()
            .map_err(|error| invalid(&path, &format!("invalid controller: {error:?}")))?;
        self.project.controllers.insert(id.clone(), controller);
        Ok(())
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
        require_allowed_mapping_keys(
            document.path(),
            &value,
            &["type", "pixels"],
            "fixture definition",
        )?;
        let pixels = sequence_values(document.path(), &value, "pixels")?
            .iter()
            .map(|value| Self::parse_pixel(&document, value))
            .collect::<Result<Vec<_>, _>>()?;
        self.project
            .definitions
            .fixtures
            .definitions
            .insert(id.clone(), FixtureDefinition { pixels });
        Ok(())
    }

    fn fixture_reference(
        &self,
        document: &DocumentId,
        value: &Value,
        key: &str,
    ) -> Result<FixtureDefinitionId, LoadProjectError> {
        let reference = string_field(document.path(), value, key)?;
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
        require_allowed_mapping_keys(path, value, &["id", "position", "diameter"], "pixel")?;
        let diameter = required_field(path, value, "diameter")?
            .as_f64()
            .ok_or_else(|| invalid(path, "Pixel diameter must be a number."))?;
        if !diameter.is_finite() || diameter < 0.000001 || diameter > 100.0 {
            return Err(invalid(
                path,
                "Pixel diameter must be between 0.000001 and 100 meters.",
            ));
        }
        Ok(Pixel {
            id: PixelId(u32_field(path, value, "id")?),
            position: optional_field(value, "position")
                .map(|point| parse_point3(path, point))
                .transpose()?
                .unwrap_or_default(),
            diameter: donder_language::values::DistanceSpan {
                micrometers: (diameter * 1_000_000.0).round() as u32,
            },
        })
    }

    pub(super) fn resolve_layout(&mut self, id: &LayoutId) -> Result<(), LoadProjectError> {
        if self.project.layouts.contains_key(id) {
            return Ok(());
        }
        let (document, _, value) = self
            .loader
            .object_value(&ResolvedObject::Layout(id.clone()))?;
        require_allowed_mapping_keys(document.path(), &value, &["type", "fixtures"], "layout")?;
        let fixtures = sequence_values(document.path(), &value, "fixtures")?
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
    }

    fn parse_layout_fixture(
        &self,
        document: &DocumentId,
        value: &Value,
    ) -> Result<LayoutFixture, LoadProjectError> {
        let path = document.path();
        let kind = match string_field(path, value, "type")? {
            "fixture" => {
                require_allowed_mapping_keys(
                    path,
                    value,
                    &["type", "id", "name", "transform", "definition"],
                    "fixture instance",
                )?;
                LayoutFixtureKind::Fixture {
                    definition: self.fixture_reference(document, value, "definition")?,
                    transform: parse_fixture_transform(path, optional_field(value, "transform"))?,
                }
            }
            "group" => {
                require_allowed_mapping_keys(
                    path,
                    value,
                    &["type", "id", "name", "children"],
                    "layout group",
                )?;
                LayoutFixtureKind::Group {
                    children: sequence_values(path, value, "children")?
                        .iter()
                        .map(|child| self.parse_layout_fixture(document, child))
                        .collect::<Result<_, _>>()?,
                }
            }
            _ => return Err(invalid(path, "Expected fixture or group.")),
        };
        Ok(LayoutFixture {
            id: FixtureInstanceId(u32_field(path, value, "id")?),
            name: string_field(path, value, "name")?.to_owned(),
            kind,
        })
    }

    pub(super) fn resolve_patch(&mut self, id: &PatchId) -> Result<(), LoadProjectError> {
        if self.project.patches.contains_key(id) {
            return Ok(());
        }
        let (document, _, value) = self
            .loader
            .object_value(&ResolvedObject::Patch(id.clone()))?;
        require_allowed_mapping_keys(document.path(), &value, &["type", "routes"], "patch")?;
        let routes = sequence_values(document.path(), &value, "routes")?
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
    }

    fn parse_fixture_target(
        &self,
        document: &DocumentId,
        value: &Value,
    ) -> Result<FixtureTarget, LoadProjectError> {
        let path = document.path();
        require_allowed_mapping_keys(path, value, &["layout", "fixture"], "fixture target")?;
        let layout = match self
            .loader
            .resolve_reference(document, string_field(path, value, "layout")?)?
        {
            ResolvedObject::Layout(id) => id,
            _ => return Err(invalid(path, "Expected a layout reference.")),
        };
        Ok(FixtureTarget {
            layout,
            fixture: FixtureInstanceId(u32_field(path, value, "fixture")?),
        })
    }

    fn parse_pixel_route(
        &self,
        document: &DocumentId,
        value: &Value,
    ) -> Result<PixelRoute, LoadProjectError> {
        let path = document.path();
        require_allowed_mapping_keys(
            path,
            value,
            &[
                "id",
                "target",
                "pixels",
                "controller",
                "port",
                "start_slot",
                "encoding",
                "gamma",
                "brightness",
            ],
            "LED route",
        )?;
        let controller = match self
            .loader
            .resolve_reference(document, string_field(path, value, "controller")?)?
        {
            ResolvedObject::Controller(id) => id,
            _ => return Err(invalid(path, "Expected a controller reference.")),
        };
        let pixels = optional_field(value, "pixels")
            .map(|span| {
                require_allowed_mapping_keys(path, span, &["start", "count"], "output pixel span")?;
                Ok::<_, LoadProjectError>(PixelSpan {
                    start: u32_field(path, span, "start")?,
                    count: u32_field(path, span, "count")?,
                })
            })
            .transpose()?;
        let encoding = required_field(path, value, "encoding")?;
        require_allowed_mapping_keys(path, encoding, &["type", "order"], "pixel encoding")?;
        let order = sequence_values(path, encoding, "order")?
            .iter()
            .map(|channel| {
                channel
                    .as_u64()
                    .and_then(|channel| u8::try_from(channel).ok())
                    .ok_or_else(|| invalid(path, "Channel order must contain byte indices."))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let encoding = match string_field(path, encoding, "type")? {
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
        };
        Ok(PixelRoute {
            id: PixelRouteId(u32_field(path, value, "id")?),
            target: self.parse_fixture_target(document, required_field(path, value, "target")?)?,
            pixels,
            controller,
            port: ControllerPortId(u32_field(path, value, "port")?),
            start_slot: u32_field(path, value, "start_slot")?
                .try_into()
                .map_err(|_| invalid(path, "Start slot is out of range."))?,
            encoding,
            gamma: f32_field(path, value, "gamma")?,
            brightness: f32_field(path, value, "brightness")?,
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
        require_allowed_mapping_keys(
            &document_path,
            &value,
            &[
                "type",
                "duration",
                "frame_rate",
                "audio",
                "mark_collections",
                "layers",
                "effects",
                "composition_graph",
                "automation_clips",
            ],
            "sequence",
        )?;
        let duration =
            parse_duration(string_field(&document_path, &value, "duration")?).map_err(|error| {
                with_yaml_location(
                    error,
                    &document_path,
                    source_range_for_field_value(&document_path, &value, "duration"),
                )
            })?;
        let audio = self.parse_audio(&document_id, &value)?;
        let mark_collections = optional_sequence(&document_path, &value, "mark_collections")?
            .into_iter()
            .flatten()
            .map(|collection| parse_mark_collection(&document_path, collection))
            .collect::<Result<Vec<_>, _>>()?;
        let layers = sequence_values(&document_path, &value, "layers")?
            .iter()
            .map(|layer| parse_sequence_layer(&document_path, layer))
            .collect::<Result<Vec<_>, _>>()?;
        let effects = sequence_values(&document_path, &value, "effects")?
            .iter()
            .map(|effect| self.parse_sequence_effect(&document_id, effect))
            .collect::<Result<Vec<_>, _>>()?;
        let composition_graph = self.parse_composition_graph(
            &document_id,
            required_field(&document_path, &value, "composition_graph")?,
        )?;
        let automation_clips = optional_sequence(&document_path, &value, "automation_clips")?
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
                    range: source_range_for_field_value(&document_path, &value, "automation_clips"),
                    message: "sequence has duplicate automation targets".to_string(),
                });
            }
        }
        self.project.sequences.insert(
            id.clone(),
            Sequence {
                id: id.clone(),
                duration,
                frame_rate: u32_field(&document_path, &value, "frame_rate")?,
                audio,
                mark_collections,
                layers,
                effects,
                composition_graph,
                automation_clips,
            },
        );
        Ok(())
    }

    pub(super) fn parse_audio(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
    ) -> Result<SequenceAudio, LoadProjectError> {
        let path = document_id.path();
        let Some(audio) = optional_field(value, "audio") else {
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
        let definition = self.parse_effect_definition(document_id, value)?;
        let param_overrides = self.parse_param_overrides(document_id, value)?;
        Ok(EffectInst {
            id: EffectInstId(u32_field(path, value, "id")?),
            layer_id: SequenceLayerId(u32_field(path, value, "layer_id")?),
            start: parse_duration_as_time(string_field(path, value, "start")?).map_err(
                |error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "start"),
                    )
                },
            )?,
            duration: parse_duration(string_field(path, value, "duration")?).map_err(|error| {
                with_yaml_location(
                    error,
                    path,
                    source_range_for_field_value(path, value, "duration"),
                )
            })?,
            target: self
                .parse_fixture_target(document_id, required_field(path, value, "target")?)?,
            scope: parse_effect_scope(path, value)?,
            definition,
            param_overrides,
        })
    }

    pub(super) fn parse_composition_graph(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
    ) -> Result<SequenceCompositionGraph, LoadProjectError> {
        let path = document_id.path();
        let graph = SequenceCompositionGraph {
            nodes: sequence_values(path, value, "nodes")?
                .iter()
                .map(|node| self.parse_composition_graph_node(document_id, node))
                .collect::<Result<Vec<_>, _>>()?,
            edges: sequence_values(path, value, "edges")?
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
    }

    pub(super) fn parse_composition_graph_node(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
    ) -> Result<CompositionGraphNode, LoadProjectError> {
        let path = document_id.path();
        let kind = match string_field(path, value, "type")? {
            "layer" => CompositionGraphNodeKind::Layer {
                layer_id: SequenceLayerId(u32_field(path, value, "layer_id")?),
            },
            "operator" => CompositionGraphNodeKind::Operator(GraphOperatorNode {
                operator: self.parse_graph_operator_ref(document_id, value)?,
                params: self.parse_graph_operator_params(document_id, value)?,
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
            id: CompositionGraphNodeId(u32_field(path, value, "id")?),
            position: parse_graph_position(path, required_field(path, value, "position")?)?,
            kind,
        })
    }

    fn parse_effect_definition(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
    ) -> Result<EffectRef, LoadProjectError> {
        let path = document_id.path();
        let effect_ref = string_field(path, value, "effect")?;
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
        value: &Value,
    ) -> Result<IndexMap<Identifier, EffectParamValue>, LoadProjectError> {
        let path = document_id.path();
        Ok(optional_mapping(path, value, "params")?
            .map(|mapping| {
                mapping
                    .iter()
                    .map(|(key, value)| {
                        let key =
                            key.as_str()
                                .ok_or_else(|| LoadProjectError::InvalidDocument {
                                    path: path.to_path_buf(),
                                    range: None,
                                    message: "parameter keys must be strings".to_string(),
                                })?;
                        let identifier = Identifier::new(key.to_string()).map_err(|_| {
                            LoadProjectError::InvalidDocument {
                                path: path.to_path_buf(),
                                range: None,
                                message: format!("invalid parameter name `{key}`"),
                            }
                        })?;
                        Ok((identifier, self.parse_effect_param(document_id, value)?))
                    })
                    .collect::<Result<IndexMap<_, _>, LoadProjectError>>()
            })
            .transpose()?
            .unwrap_or_else(IndexMap::new))
    }

    pub(super) fn parse_graph_operator_params(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
    ) -> Result<IndexMap<Identifier, EffectParamValue>, LoadProjectError> {
        self.parse_param_overrides(document_id, value)
    }

    pub(super) fn parse_graph_operator_ref(
        &self,
        document_id: &donder_language::identity::DocumentId,
        value: &Value,
    ) -> Result<OperatorRef, LoadProjectError> {
        let path = document_id.path();
        let name = string_field(path, value, "operator")?;
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
        let kind = string_field(path, value, "type")?;
        let fields: &[&str] = match kind {
            "integer" | "float" | "bool" | "color" | "enum" => &["type", "value"],
            "marks" => &["type", "key"],
            "curve" => &["type", "curve"],
            "gradient" => &["type", "gradient"],
            "array" => &["type", "values"],
            other => {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, "type"),
                    message: format!("unsupported effect param type `{other}`"),
                });
            }
        };
        require_allowed_mapping_keys(path, value, fields, "effect parameter value")?;
        match kind {
            "integer" => Ok(EffectParamValue::Int(i32_field(path, value, "value")?)),
            "float" => Ok(EffectParamValue::Float(f32_field(path, value, "value")?)),
            "bool" => Ok(EffectParamValue::Bool(bool_field(path, value, "value")?)),
            "color" => Ok(EffectParamValue::Color(
                parse_color(string_field(path, value, "value")?).map_err(|error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "value"),
                    )
                })?,
            )),
            "enum" => Ok(EffectParamValue::Enum(
                Identifier::new(string_field(path, value, "value")?.to_string()).map_err(|_| {
                    LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "invalid enum value".to_string(),
                    }
                })?,
            )),
            "marks" => Ok(EffectParamValue::Marks(MarkCollectionKey {
                name: string_field(path, value, "key")?.to_string(),
            })),
            "curve" => Ok(EffectParamValue::Curve(self.parse_curve_source(
                document_id,
                required_field(path, value, "curve")?,
            )?)),
            "gradient" => Ok(EffectParamValue::Gradient(self.parse_gradient_source(
                document_id,
                required_field(path, value, "gradient")?,
            )?)),
            "array" => {
                let values = sequence_values(path, value, "values")?
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
        if optional_field(value, "type").is_some() {
            return self.parse_effect_param(document_id, value);
        }
        if let Some(curve) = optional_field(value, "curve") {
            require_allowed_mapping_keys(path, value, &["curve"], "curve array item")?;
            return Ok(EffectParamValue::Curve(
                self.parse_curve_source(document_id, curve)?,
            ));
        }
        let gradient = required_field(path, value, "gradient")?;
        require_allowed_mapping_keys(path, value, &["gradient"], "gradient array item")?;
        Ok(EffectParamValue::Gradient(
            self.parse_gradient_source(document_id, gradient)?,
        ))
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
        if let Some(curve_value) = optional_field(value, "curve") {
            return self.parse_curve_source(document_id, curve_value);
        }
        Ok(CurveSource::Inline(parse_curve(path, value)?))
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
        if let Some(gradient) = optional_field(value, "gradient") {
            return self.parse_gradient_source(document_id, gradient);
        }
        Ok(GradientSource::Inline(parse_gradient(path, value)?))
    }

    pub(super) fn parse_automation_clip(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<AutomationClip, LoadProjectError> {
        let bindings = sequence_values(path, value, "bindings")?
            .iter()
            .map(|binding| parse_automation_binding(path, binding))
            .collect::<Result<Vec<_>, _>>()?;
        let detached_bindings = optional_sequence(path, value, "detached_bindings")?
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
                    message: "automation clip has duplicate bindings for a parameter".to_string(),
                });
            }
        }
        Ok(AutomationClip {
            id: AutomationClipId(u32_field(path, value, "id")?),
            start: parse_duration_as_time(string_field(path, value, "start")?).map_err(
                |error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "start"),
                    )
                },
            )?,
            duration: parse_duration(string_field(path, value, "duration")?).map_err(|error| {
                with_yaml_location(
                    error,
                    path,
                    source_range_for_field_value(path, value, "duration"),
                )
            })?,
            anchor_lane_index: u32_field(path, value, "anchor_lane_index")?,
            lane_index: u32_field(path, value, "lane_index")?,
            curve: parse_automation_curve(path, required_field(path, value, "curve")?)?,
            bindings,
            detached_bindings,
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
    ResolvedObject, bool_field, f32_field, i32_field, optional_field, optional_mapping,
    optional_sequence, parse_automation_binding, parse_automation_curve, parse_color, parse_curve,
    parse_detached_automation_binding, parse_duration, parse_duration_as_time, parse_effect_scope,
    parse_gradient, parse_graph_edge, parse_graph_position, parse_mark_collection, parse_point3,
    parse_rotation3, parse_scale3, parse_sequence_layer, require_allowed_mapping_keys,
    required_field, sequence_field, sequence_values, string_field, u32_field,
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
    require_allowed_mapping_keys(
        path,
        value,
        &["position", "rotation", "scale"],
        "fixture transform",
    )?;
    Ok(FixtureTransform {
        position: optional_field(value, "position")
            .map(|point| parse_point3(path, point))
            .transpose()?
            .unwrap_or_default(),
        rotation: optional_field(value, "rotation")
            .map(|rotation| parse_rotation3(path, rotation))
            .transpose()?
            .unwrap_or_default(),
        scale: optional_field(value, "scale")
            .map(|scale| parse_scale3(path, scale))
            .transpose()?
            .unwrap_or_default(),
    })
}
