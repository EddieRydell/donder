pub(super) struct DomainResolver<'a> {
    pub(super) loader: &'a mut Loader,
    pub(super) project: &'a mut DawnProject,
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
            &["type", "elements", "preview", "patch", "controllers"],
            "setup",
        )?;
        let elements_ref = string_field(&document_path, &value, "elements")?;
        let preview_ref = string_field(&document_path, &value, "preview")?;
        let patch_ref = string_field(&document_path, &value, "patch")?;
        let elements = match self.loader.resolve_reference(&document_id, elements_ref)? {
            ResolvedObject::ElementTree(elements) => elements,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: document_path.clone(),
                    range: source_range_for_scalar(&document_path, elements_ref),
                    reference: elements_ref.to_string(),
                });
            }
        };
        let preview = match self.loader.resolve_reference(&document_id, preview_ref)? {
            ResolvedObject::PreviewLayout(preview) => preview,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: document_path.clone(),
                    range: source_range_for_scalar(&document_path, preview_ref),
                    reference: preview_ref.to_string(),
                });
            }
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
                elements: elements.clone(),
                preview: preview.clone(),
                patch: patch.clone(),
                controllers: controllers.clone(),
            },
        );
        self.resolve_element_tree(&elements)?;
        self.resolve_preview_layout(&preview)?;
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

    pub(super) fn resolve_element_tree(
        &mut self,
        id: &ElementTreeId,
    ) -> Result<(), LoadProjectError> {
        if self.project.element_trees.contains_key(id) {
            return Ok(());
        }
        let (document_id, _, value) = self
            .loader
            .object_value(&ResolvedObject::ElementTree(id.clone()))?;
        let document_path = document_id.path().to_path_buf();
        require_allowed_mapping_keys(
            &document_path,
            &value,
            &["type", "roots", "nodes"],
            "element tree",
        )?;
        let roots = sequence_values(&document_path, &value, "roots")?
            .iter()
            .map(|root| {
                root.as_u64()
                    .and_then(|raw| u32::try_from(raw).ok())
                    .map(ElementNodeId)
                    .ok_or_else(|| invalid(&document_path, "element roots must be u32 ids"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut nodes = IndexMap::new();
        for node in sequence_values(&document_path, &value, "nodes")? {
            let fields: &[&str] = match string_field(&document_path, node, "type")? {
                "group" => &["id", "name", "type", "children"],
                "color" => &["id", "name", "type", "cells", "capability"],
                "scalar" => &["id", "name", "type", "cells"],
                "indexed" => &["id", "name", "type", "cells", "options"],
                "fixture" => &["id", "name", "type", "profile"],
                other => {
                    return Err(invalid(
                        &document_path,
                        &format!("invalid element node type `{other}`"),
                    ));
                }
            };
            require_allowed_mapping_keys(&document_path, node, fields, "element node")?;
            let node_id = ElementNodeId(u32_field(&document_path, node, "id")?);
            let name = string_field(&document_path, node, "name")?.to_string();
            let kind = match string_field(&document_path, node, "type")? {
                "group" => ElementNodeKind::Group {
                    children: sequence_values(&document_path, node, "children")?
                        .iter()
                        .map(|child| {
                            child
                                .as_u64()
                                .and_then(|raw| u32::try_from(raw).ok())
                                .map(ElementNodeId)
                                .ok_or_else(|| {
                                    invalid(&document_path, "element children must be u32 ids")
                                })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                },
                "color" => ElementNodeKind::Color {
                    cells: u32_field(&document_path, node, "cells")?,
                    capability: self.parse_color_capability(
                        &document_path,
                        required_field(&document_path, node, "capability")?,
                    )?,
                },
                "scalar" => ElementNodeKind::Scalar {
                    cells: u32_field(&document_path, node, "cells")?,
                },
                "indexed" => ElementNodeKind::Indexed {
                    cells: u32_field(&document_path, node, "cells")?,
                    options: sequence_values(&document_path, node, "options")?
                        .iter()
                        .map(|option| {
                            require_allowed_mapping_keys(
                                &document_path,
                                option,
                                &["id", "name"],
                                "indexed option",
                            )?;
                            Ok(IndexedOption {
                                id: IndexedOptionId(u32_field(&document_path, option, "id")?),
                                name: string_field(&document_path, option, "name")?.to_string(),
                            })
                        })
                        .collect::<Result<Vec<_>, LoadProjectError>>()?,
                },
                "fixture" => {
                    let reference = string_field(&document_path, node, "profile")?;
                    let profile = match self.loader.resolve_reference(&document_id, reference)? {
                        ResolvedObject::FixtureProfile(profile) => profile,
                        _ => {
                            return Err(LoadProjectError::InvalidReference {
                                path: document_path.clone(),
                                range: source_range_for_scalar(&document_path, reference),
                                reference: reference.to_string(),
                            });
                        }
                    };
                    self.resolve_fixture_profile(&profile)?;
                    ElementNodeKind::Fixture { profile }
                }
                other => {
                    return Err(invalid(
                        &document_path,
                        &format!("invalid element node type `{other}`"),
                    ));
                }
            };
            if nodes.insert(node_id, ElementNode { name, kind }).is_some() {
                return Err(invalid(&document_path, "duplicate element node id"));
            }
        }
        let tree = ElementTree {
            id: id.clone(),
            roots,
            nodes,
        };
        tree.validate().map_err(|error| {
            invalid(&document_path, &format!("invalid element tree: {error:?}"))
        })?;
        self.project.element_trees.insert(id.clone(), tree);
        Ok(())
    }

    pub(super) fn parse_prop_instance(
        &self,
        document_id: &dawn_language::identity::DocumentId,
        value: &Value,
    ) -> Result<PropInstance, LoadProjectError> {
        let path = document_id.path();
        require_allowed_mapping_keys(
            path,
            value,
            &["id", "name", "prop", "transform", "bindings"],
            "preview prop",
        )?;
        let id = PropInstanceId(u32_field(path, value, "id")?);
        let name = string_field(path, value, "name")?.to_string();
        let prop_ref = string_field(path, value, "prop")?;
        let definition = match self.loader.resolve_reference(document_id, prop_ref)? {
            ResolvedObject::PropDefinition(definition) => definition,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    range: source_range_for_scalar(path, prop_ref),
                    reference: prop_ref.to_string(),
                });
            }
        };
        let transform = required_field(path, value, "transform")?;
        require_allowed_mapping_keys(
            path,
            transform,
            &["position", "rotation", "scale"],
            "preview transform",
        )?;
        let bindings = sequence_values(path, value, "bindings")?
            .iter()
            .map(|binding| {
                require_allowed_mapping_keys(path, binding, &["node", "cell"], "preview binding")?;
                Ok(ElementCellAddress {
                    node: ElementNodeId(u32_field(path, binding, "node")?),
                    cell: u32_field(path, binding, "cell")?,
                })
            })
            .collect::<Result<Vec<_>, LoadProjectError>>()?;
        Ok(PropInstance {
            id,
            name,
            definition,
            position: parse_point3(path, required_field(path, transform, "position")?)?,
            rotation: parse_rotation3(path, required_field(path, transform, "rotation")?)?,
            scale: parse_scale3(path, required_field(path, transform, "scale")?)?,
            bindings,
        })
    }

    pub(super) fn resolve_fixture_profile(
        &mut self,
        id: &FixtureProfileId,
    ) -> Result<(), LoadProjectError> {
        if self
            .project
            .definitions
            .fixture_profiles
            .definitions
            .contains_key(id)
        {
            return Ok(());
        }
        let (document_id, _, value) = self
            .loader
            .object_value(&ResolvedObject::FixtureProfile(id.clone()))?;
        let path = document_id.path().to_path_buf();
        let profile = self.parse_fixture_profile(&path, id.clone(), &value)?;
        profile
            .validate()
            .map_err(|error| invalid(&path, &format!("invalid fixture profile: {error:?}")))?;
        self.project
            .definitions
            .fixture_profiles
            .definitions
            .insert(id.clone(), profile);
        Ok(())
    }

    pub(super) fn resolve_preview_layout(
        &mut self,
        id: &PreviewLayoutId,
    ) -> Result<(), LoadProjectError> {
        if self.project.preview_layouts.contains_key(id) {
            return Ok(());
        }
        let (document_id, _, value) = self
            .loader
            .object_value(&ResolvedObject::PreviewLayout(id.clone()))?;
        let path = document_id.path().to_path_buf();
        require_allowed_mapping_keys(
            &path,
            &value,
            &["type", "element_tree", "props"],
            "preview layout",
        )?;
        let tree_ref = string_field(&path, &value, "element_tree")?;
        let element_tree = match self.loader.resolve_reference(&document_id, tree_ref)? {
            ResolvedObject::ElementTree(tree) => tree,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.clone(),
                    range: source_range_for_scalar(&path, tree_ref),
                    reference: tree_ref.to_string(),
                });
            }
        };
        self.resolve_element_tree(&element_tree)?;
        let props = sequence_values(&path, &value, "props")?
            .iter()
            .map(|prop| self.parse_prop_instance(&document_id, prop))
            .collect::<Result<Vec<_>, _>>()?;
        self.project.preview_layouts.insert(
            id.clone(),
            PreviewLayout {
                id: id.clone(),
                element_tree,
                props,
            },
        );
        Ok(())
    }

    pub(super) fn resolve_patch(&mut self, id: &PatchId) -> Result<(), LoadProjectError> {
        if self.project.patches.contains_key(id) {
            return Ok(());
        }
        let (document_id, _, value) = self
            .loader
            .object_value(&ResolvedObject::Patch(id.clone()))?;
        let document_path = document_id.path().to_path_buf();
        require_allowed_mapping_keys(&document_path, &value, &["type", "nodes", "edges"], "patch")?;
        let mut nodes = IndexMap::new();
        for node in sequence_values(&document_path, &value, "nodes")? {
            let node_id = PatchNodeId(u32_field(&document_path, node, "id")?);
            let parsed = match string_field(&document_path, node, "type")? {
                "source" => PatchNode::Source(self.parse_patch_source(&document_id, node)?),
                "filter" => PatchNode::Filter(self.parse_filter(&document_id, node)?),
                "sink" => {
                    require_allowed_mapping_keys(
                        &document_path,
                        node,
                        &[
                            "id",
                            "type",
                            "controller",
                            "port",
                            "start_slot",
                            "slot_count",
                        ],
                        "patch sink",
                    )?;
                    let reference = string_field(&document_path, node, "controller")?;
                    let controller = match self.loader.resolve_reference(&document_id, reference)? {
                        ResolvedObject::Controller(controller) => controller,
                        _ => {
                            return Err(LoadProjectError::InvalidReference {
                                path: document_path.clone(),
                                range: source_range_for_scalar(&document_path, reference),
                                reference: reference.to_string(),
                            });
                        }
                    };
                    self.resolve_controller(&controller)?;
                    PatchNode::Sink(PatchSink {
                        controller,
                        port: ControllerPortId(u32_field(&document_path, node, "port")?),
                        start_slot: u16::try_from(u32_field(&document_path, node, "start_slot")?)
                            .map_err(|_| {
                            invalid(&document_path, "start_slot must be a u16")
                        })?,
                        slot_count: u16::try_from(u32_field(&document_path, node, "slot_count")?)
                            .map_err(|_| {
                            invalid(&document_path, "slot_count must be a u16")
                        })?,
                    })
                }
                other => {
                    return Err(invalid(
                        &document_path,
                        &format!("invalid patch node type `{other}`"),
                    ));
                }
            };
            if nodes.insert(node_id, parsed).is_some() {
                return Err(invalid(&document_path, "duplicate patch node id"));
            }
        }
        let edges = sequence_values(&document_path, &value, "edges")?
            .iter()
            .map(|edge| {
                require_allowed_mapping_keys(
                    &document_path,
                    edge,
                    &["from", "from_port", "to", "to_port"],
                    "patch edge",
                )?;
                Ok(PatchEdge {
                    from: PatchNodeId(u32_field(&document_path, edge, "from")?),
                    from_port: PatchPortId(
                        u16::try_from(u32_field(&document_path, edge, "from_port")?)
                            .map_err(|_| invalid(&document_path, "from_port must be a u16"))?,
                    ),
                    to: PatchNodeId(u32_field(&document_path, edge, "to")?),
                    to_port: PatchPortId(
                        u16::try_from(u32_field(&document_path, edge, "to_port")?)
                            .map_err(|_| invalid(&document_path, "to_port must be a u16"))?,
                    ),
                })
            })
            .collect::<Result<Vec<_>, LoadProjectError>>()?;
        let graph = PatchGraph {
            id: id.clone(),
            nodes,
            edges,
        };
        graph
            .validate()
            .map_err(|error| invalid(&document_path, &format!("invalid patch graph: {error:?}")))?;
        self.project.patches.insert(id.clone(), graph);
        Ok(())
    }

    pub(super) fn parse_patch_source(
        &self,
        document_id: &dawn_language::identity::DocumentId,
        value: &Value,
    ) -> Result<PatchSource, LoadProjectError> {
        let path = document_id.path();
        let fields: &[&str] = if string_field(path, value, "output")? == "fixture_state" {
            &["id", "type", "selection", "width", "output", "profile"]
        } else {
            &["id", "type", "selection", "width", "output"]
        };
        require_allowed_mapping_keys(path, value, fields, "patch source")?;
        let selection =
            self.parse_element_selection(document_id, required_field(path, value, "selection")?)?;
        let width = usize_field(path, value, "width")?;
        let output = match string_field(path, value, "output")? {
            "color" => PatchValueType::Color { width },
            "scalar" => PatchValueType::Scalar { width },
            "indexed" => PatchValueType::Indexed { width },
            "fixture_state" => {
                let reference = string_field(path, value, "profile")?;
                let profile = match self.loader.resolve_reference(document_id, reference)? {
                    ResolvedObject::FixtureProfile(profile) => profile,
                    _ => {
                        return Err(LoadProjectError::InvalidReference {
                            path: path.to_path_buf(),
                            range: source_range_for_scalar(path, reference),
                            reference: reference.to_string(),
                        });
                    }
                };
                PatchValueType::FixtureState { width, profile }
            }
            other => {
                return Err(invalid(
                    path,
                    &format!("invalid patch source output `{other}`"),
                ));
            }
        };
        Ok(PatchSource { selection, output })
    }

    fn parse_element_selection(
        &self,
        document_id: &dawn_language::identity::DocumentId,
        value: &Value,
    ) -> Result<ElementSelection, LoadProjectError> {
        let path = document_id.path();
        require_allowed_mapping_keys(path, value, &["tree", "node", "cells"], "element selection")?;
        let reference = string_field(path, value, "tree")?;
        let tree = match self.loader.resolve_reference(document_id, reference)? {
            ResolvedObject::ElementTree(tree) => tree,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    range: source_range_for_scalar(path, reference),
                    reference: reference.to_string(),
                });
            }
        };
        let cells = optional_field(value, "cells")
            .map(|range| {
                require_allowed_mapping_keys(
                    path,
                    range,
                    &["start", "count"],
                    "element cell range",
                )?;
                Ok(ElementCellRange {
                    start: u32_field(path, range, "start")?,
                    count: u32_field(path, range, "count")?,
                })
            })
            .transpose()?;
        Ok(ElementSelection {
            tree,
            node: ElementNodeId(u32_field(path, value, "node")?),
            cells,
        })
    }

    fn parse_color_capability(
        &self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<ColorCapability, LoadProjectError> {
        let fields: &[&str] = if string_field(path, value, "type")? == "discrete" {
            &["type", "emitters", "mappings"]
        } else {
            &["type"]
        };
        require_allowed_mapping_keys(path, value, fields, "color capability")?;
        match string_field(path, value, "type")? {
            "rgb" => Ok(ColorCapability::Rgb),
            "rgbw" => Ok(ColorCapability::Rgbw),
            "discrete" => {
                let emitters = sequence_values(path, value, "emitters")?
                    .iter()
                    .map(|emitter| {
                        require_allowed_mapping_keys(
                            path,
                            emitter,
                            &["id", "name"],
                            "discrete emitter",
                        )?;
                        Ok(DiscreteEmitter {
                            id: EmitterId(u32_field(path, emitter, "id")?),
                            name: string_field(path, emitter, "name")?.to_string(),
                        })
                    })
                    .collect::<Result<Vec<_>, LoadProjectError>>()?;
                let mappings = sequence_values(path, value, "mappings")?
                    .iter()
                    .map(|mapping| {
                        require_allowed_mapping_keys(
                            path,
                            mapping,
                            &["color", "levels"],
                            "discrete color mapping",
                        )?;
                        let mut levels = IndexMap::new();
                        for level in sequence_values(path, mapping, "levels")? {
                            require_allowed_mapping_keys(
                                path,
                                level,
                                &["emitter", "level"],
                                "discrete emitter level",
                            )?;
                            if levels
                                .insert(
                                    EmitterId(u32_field(path, level, "emitter")?),
                                    f32_field(path, level, "level")?,
                                )
                                .is_some()
                            {
                                return Err(invalid(path, "duplicate emitter level id"));
                            }
                        }
                        Ok(DiscreteColorMapping {
                            color: parse_color(string_field(path, mapping, "color")?)?,
                            levels,
                        })
                    })
                    .collect::<Result<Vec<_>, LoadProjectError>>()?;
                Ok(ColorCapability::Discrete { emitters, mappings })
            }
            other => Err(invalid(
                path,
                &format!("invalid color capability `{other}`"),
            )),
        }
    }

    fn parse_filter(
        &self,
        document_id: &dawn_language::identity::DocumentId,
        value: &Value,
    ) -> Result<FilterDefinition, LoadProjectError> {
        let path = document_id.path();
        let fields: &[&str] = match string_field(path, value, "filter")? {
            "color_breakdown" => &["id", "type", "filter", "capability", "cell_count"],
            "dimming_curve" => &["id", "type", "filter", "curve", "width"],
            "scale_invert" => &["id", "type", "filter", "scale", "invert", "width"],
            "fan_out" => &["id", "type", "filter", "width", "outputs"],
            "component_reorder" => &[
                "id",
                "type",
                "filter",
                "components_per_cell",
                "order",
                "cell_count",
            ],
            "indexed_value_mapping" => &["id", "type", "filter", "entries", "width"],
            "scalar_to_components" | "quantize_8" => &["id", "type", "filter", "width"],
            "quantize_16" => &["id", "type", "filter", "width", "byte_order"],
            "fixture_profile_encoding" => &[
                "id",
                "type",
                "filter",
                "profile",
                "fixture_count",
                "slot_count",
            ],
            other => return Err(invalid(path, &format!("invalid patch filter `{other}`"))),
        };
        require_allowed_mapping_keys(path, value, fields, "patch filter")?;
        Ok(match string_field(path, value, "filter")? {
            "color_breakdown" => FilterDefinition::ColorBreakdown {
                capability: self
                    .parse_color_capability(path, required_field(path, value, "capability")?)?,
                cell_count: usize_field(path, value, "cell_count")?,
            },
            "dimming_curve" => FilterDefinition::DimmingCurve {
                curve: parse_dimming_curve(path, required_field(path, value, "curve")?)?,
                width: usize_field(path, value, "width")?,
            },
            "scale_invert" => FilterDefinition::ScaleInvert {
                scale: f32_field(path, value, "scale")?,
                invert: bool_field(path, value, "invert")?,
                width: usize_field(path, value, "width")?,
            },
            "fan_out" => FilterDefinition::FanOut {
                width: usize_field(path, value, "width")?,
                outputs: u16::try_from(u32_field(path, value, "outputs")?)
                    .map_err(|_| invalid(path, "fan-out outputs must be a u16"))?,
            },
            "component_reorder" => FilterDefinition::ComponentReorder {
                components_per_cell: u16::try_from(u32_field(path, value, "components_per_cell")?)
                    .map_err(|_| invalid(path, "components_per_cell must be a u16"))?,
                order: sequence_values(path, value, "order")?
                    .iter()
                    .map(|item| {
                        item.as_u64()
                            .and_then(|raw| u16::try_from(raw).ok())
                            .ok_or_else(|| invalid(path, "component order values must be u16"))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                cell_count: usize_field(path, value, "cell_count")?,
            },
            "indexed_value_mapping" => {
                let mut entries = IndexMap::new();
                for entry in sequence_values(path, value, "entries")? {
                    require_allowed_mapping_keys(
                        path,
                        entry,
                        &["id", "value"],
                        "indexed value mapping",
                    )?;
                    if entries
                        .insert(
                            u32_field(path, entry, "id")?,
                            f32_field(path, entry, "value")?,
                        )
                        .is_some()
                    {
                        return Err(invalid(path, "duplicate indexed mapping id"));
                    }
                }
                FilterDefinition::IndexedValueMapping {
                    entries,
                    width: usize_field(path, value, "width")?,
                }
            }
            "scalar_to_components" => FilterDefinition::ScalarToComponents {
                width: usize_field(path, value, "width")?,
            },
            "quantize_8" => FilterDefinition::Quantize8 {
                width: usize_field(path, value, "width")?,
            },
            "quantize_16" => FilterDefinition::Quantize16 {
                width: usize_field(path, value, "width")?,
                byte_order: match string_field(path, value, "byte_order")? {
                    "coarse_fine" => ByteOrder::CoarseFine,
                    "fine_coarse" => ByteOrder::FineCoarse,
                    other => return Err(invalid(path, &format!("invalid byte order `{other}`"))),
                },
            },
            "fixture_profile_encoding" => {
                let reference = string_field(path, value, "profile")?;
                let profile = match self.loader.resolve_reference(document_id, reference)? {
                    ResolvedObject::FixtureProfile(profile) => profile,
                    _ => {
                        return Err(LoadProjectError::InvalidReference {
                            path: path.to_path_buf(),
                            range: source_range_for_scalar(path, reference),
                            reference: reference.to_string(),
                        });
                    }
                };
                FilterDefinition::FixtureProfileEncoding {
                    profile,
                    fixture_count: usize_field(path, value, "fixture_count")?,
                    slot_count: usize_field(path, value, "slot_count")?,
                }
            }
            other => return Err(invalid(path, &format!("invalid patch filter `{other}`"))),
        })
    }

    fn parse_fixture_profile(
        &self,
        path: &Utf8Path,
        id: FixtureProfileId,
        value: &Value,
    ) -> Result<FixtureProfile, LoadProjectError> {
        require_allowed_mapping_keys(
            path,
            value,
            &["type", "functions", "channels", "behavior_rules"],
            "fixture profile",
        )?;
        let mut functions = IndexMap::new();
        for function in sequence_values(path, value, "functions")? {
            let fields: &[&str] = match string_field(path, function, "type")? {
                "range" => &["id", "name", "type", "tag", "curve"],
                "indexed" | "color_wheel" => &["id", "name", "type", "tag", "curve", "entries"],
                "color_mixing" => &["id", "name", "type", "tag", "curve", "model"],
                other => {
                    return Err(invalid(
                        path,
                        &format!("invalid fixture function `{other}`"),
                    ));
                }
            };
            require_allowed_mapping_keys(path, function, fields, "fixture function")?;
            let function_id = FixtureFunctionId(u32_field(path, function, "id")?);
            let kind = match string_field(path, function, "type")? {
                "range" => FixtureFunctionKind::Range,
                "indexed" => FixtureFunctionKind::Indexed {
                    entries: parse_fixture_entries(path, function)?,
                },
                "color_wheel" => FixtureFunctionKind::ColorWheel {
                    entries: parse_fixture_entries(path, function)?,
                },
                "color_mixing" => FixtureFunctionKind::ColorMixing {
                    model: match string_field(path, function, "model")? {
                        "rgb" => ColorMixingModel::Rgb,
                        "rgbw" => ColorMixingModel::Rgbw,
                        other => {
                            return Err(invalid(
                                path,
                                &format!("invalid color-mixing model `{other}`"),
                            ));
                        }
                    },
                },
                other => {
                    return Err(invalid(
                        path,
                        &format!("invalid fixture function `{other}`"),
                    ));
                }
            };
            let tag = optional_string_field(function, "tag")
                .map(parse_function_tag)
                .transpose()
                .map_err(|message| invalid(path, &message))?;
            if functions
                .insert(
                    function_id,
                    FixtureFunction {
                        name: string_field(path, function, "name")?.to_string(),
                        tag,
                        kind,
                        curve: parse_dimming_curve(path, required_field(path, function, "curve")?)?,
                    },
                )
                .is_some()
            {
                return Err(invalid(path, "duplicate fixture function id"));
            }
        }
        let channels = sequence_values(path, value, "channels")?
            .iter()
            .map(|channel| {
                let fields: &[&str] = match string_field(path, channel, "role")? {
                    "ignored" => &["slot", "role", "curve"],
                    "coarse" | "fine" => &["slot", "role", "curve", "function"],
                    "color_component" => &["slot", "role", "curve", "function", "component"],
                    other => {
                        return Err(invalid(
                            path,
                            &format!("invalid fixture channel role `{other}`"),
                        ));
                    }
                };
                require_allowed_mapping_keys(path, channel, fields, "fixture channel")?;
                let role = match string_field(path, channel, "role")? {
                    "coarse" => FixtureChannelRole::Coarse {
                        function: FixtureFunctionId(u32_field(path, channel, "function")?),
                    },
                    "fine" => FixtureChannelRole::Fine {
                        function: FixtureFunctionId(u32_field(path, channel, "function")?),
                    },
                    "color_component" => FixtureChannelRole::ColorComponent {
                        function: FixtureFunctionId(u32_field(path, channel, "function")?),
                        component: parse_color_component(string_field(path, channel, "component")?)
                            .ok_or_else(|| invalid(path, "invalid color component"))?,
                    },
                    "ignored" => FixtureChannelRole::Ignored,
                    other => {
                        return Err(invalid(
                            path,
                            &format!("invalid fixture channel role `{other}`"),
                        ));
                    }
                };
                Ok(FixtureChannel {
                    slot: u16::try_from(u32_field(path, channel, "slot")?)
                        .map_err(|_| invalid(path, "fixture slot must be a u16"))?,
                    role,
                    curve: parse_dimming_curve(path, required_field(path, channel, "curve")?)?,
                })
            })
            .collect::<Result<Vec<_>, LoadProjectError>>()?;
        let behavior_rules = sequence_values(path, value, "behavior_rules")?
            .iter()
            .map(|rule| {
                let fields: &[&str] = match string_field(path, rule, "type")? {
                    "shutter" => &["type", "function", "closed", "open"],
                    "dimmer" => &["type", "function", "off", "on"],
                    "color_wheel" => &["type", "function", "entries"],
                    "prism_gate" => &["type", "function", "disabled", "enabled"],
                    other => {
                        return Err(invalid(
                            path,
                            &format!("invalid fixture behavior rule `{other}`"),
                        ));
                    }
                };
                require_allowed_mapping_keys(path, rule, fields, "fixture behavior rule")?;
                Ok(match string_field(path, rule, "type")? {
                    "shutter" => FixtureBehaviorRule::Shutter {
                        function: FixtureFunctionId(u32_field(path, rule, "function")?),
                        closed: FixtureEntryId(u32_field(path, rule, "closed")?),
                        open: FixtureEntryId(u32_field(path, rule, "open")?),
                    },
                    "dimmer" => FixtureBehaviorRule::Dimmer {
                        function: FixtureFunctionId(u32_field(path, rule, "function")?),
                        off: f32_field(path, rule, "off")?,
                        on: f32_field(path, rule, "on")?,
                    },
                    "color_wheel" => FixtureBehaviorRule::ColorWheel {
                        function: FixtureFunctionId(u32_field(path, rule, "function")?),
                        entries: sequence_values(path, rule, "entries")?
                            .iter()
                            .map(|entry| {
                                require_allowed_mapping_keys(
                                    path,
                                    entry,
                                    &["color", "entry"],
                                    "color wheel mapping",
                                )?;
                                Ok(ColorWheelColorMapping {
                                    color: parse_color(string_field(path, entry, "color")?)?,
                                    entry: FixtureEntryId(u32_field(path, entry, "entry")?),
                                })
                            })
                            .collect::<Result<Vec<_>, LoadProjectError>>()?,
                    },
                    "prism_gate" => FixtureBehaviorRule::PrismGate {
                        function: FixtureFunctionId(u32_field(path, rule, "function")?),
                        disabled: FixtureEntryId(u32_field(path, rule, "disabled")?),
                        enabled: FixtureEntryId(u32_field(path, rule, "enabled")?),
                    },
                    other => {
                        return Err(invalid(
                            path,
                            &format!("invalid fixture behavior rule `{other}`"),
                        ));
                    }
                })
            })
            .collect::<Result<Vec<_>, LoadProjectError>>()?;
        Ok(FixtureProfile {
            id,
            functions,
            channels,
            behavior_rules,
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
                "control_clips",
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
        let control_clips = optional_sequence(&document_path, &value, "control_clips")?
            .into_iter()
            .flatten()
            .map(|clip| self.parse_control_clip(&document_id, clip))
            .collect::<Result<Vec<_>, _>>()?;
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
                control_clips,
            },
        );
        Ok(())
    }

    pub(super) fn parse_audio(
        &mut self,
        document_id: &dawn_language::identity::DocumentId,
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
                message: format!("audio asset `{audio_path}` is not declared in dawn-package.json"),
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
        document_id: &dawn_language::identity::DocumentId,
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
                .parse_element_selection(document_id, required_field(path, value, "target")?)?,
            scope: parse_effect_scope(path, value)?,
            definition,
            param_overrides,
        })
    }

    pub(super) fn parse_composition_graph(
        &mut self,
        document_id: &dawn_language::identity::DocumentId,
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
        document_id: &dawn_language::identity::DocumentId,
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
        document_id: &dawn_language::identity::DocumentId,
        value: &Value,
    ) -> Result<EffectRef, LoadProjectError> {
        let path = document_id.path();
        let effect_ref = string_field(path, value, "effect")?;
        let reference = dawn_language::imports::SourceReference::parse(effect_ref)
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
        document_id: &dawn_language::identity::DocumentId,
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
        document_id: &dawn_language::identity::DocumentId,
        value: &Value,
    ) -> Result<IndexMap<Identifier, EffectParamValue>, LoadProjectError> {
        self.parse_param_overrides(document_id, value)
    }

    pub(super) fn parse_graph_operator_ref(
        &self,
        document_id: &dawn_language::identity::DocumentId,
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
        document_id: &dawn_language::identity::DocumentId,
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
        document_id: &dawn_language::identity::DocumentId,
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
        document_id: &dawn_language::identity::DocumentId,
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
        document_id: &dawn_language::identity::DocumentId,
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

    fn parse_control_clip(
        &self,
        document_id: &dawn_language::identity::DocumentId,
        value: &Value,
    ) -> Result<ControlClip, LoadProjectError> {
        let path = document_id.path();
        let selection =
            self.parse_element_selection(document_id, required_field(path, value, "selection")?)?;
        let target = match string_field(path, value, "target_type")? {
            "scalar" => ControlTarget::Scalar(selection),
            "indexed" => ControlTarget::Indexed(selection),
            "fixture_function" => ControlTarget::FixtureFunction {
                selection,
                function: FixtureFunctionId(u32_field(path, value, "function")?),
            },
            other => return Err(invalid(path, &format!("invalid control target `{other}`"))),
        };
        let control = required_field(path, value, "value")?;
        let value_kind = match string_field(path, control, "type")? {
            "constant_normalized" => {
                ControlValue::ConstantNormalized(f32_field(path, control, "value")?)
            }
            "normalized_curve" => ControlValue::NormalizedCurve(parse_curve(
                path,
                required_field(path, control, "curve")?,
            )?),
            "indexed" => ControlValue::Indexed {
                option: IndexedOptionId(u32_field(path, control, "option")?),
                range_curve: optional_field(control, "range_curve")
                    .map(|curve| parse_curve(path, curve))
                    .transpose()?,
            },
            "fixture_indexed" => ControlValue::FixtureIndexed {
                entry: FixtureEntryId(u32_field(path, control, "entry")?),
                range_curve: optional_field(control, "range_curve")
                    .map(|curve| parse_curve(path, curve))
                    .transpose()?,
            },
            "constant_color" => {
                ControlValue::ConstantColor(parse_color(string_field(path, control, "color")?)?)
            }
            "gradient" => ControlValue::Gradient(parse_gradient(
                path,
                required_field(path, control, "gradient")?,
            )?),
            other => return Err(invalid(path, &format!("invalid control value `{other}`"))),
        };
        Ok(ControlClip {
            id: ControlClipId(u32_field(path, value, "id")?),
            start: parse_duration_as_time(string_field(path, value, "start")?)?,
            duration: parse_duration(string_field(path, value, "duration")?)?,
            target,
            value: value_kind,
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

fn parse_dimming_curve(path: &Utf8Path, value: &Value) -> Result<DimmingCurve, LoadProjectError> {
    let fields: &[&str] = match string_field(path, value, "type")? {
        "linear" => &["type"],
        "gamma" => &["type", "value"],
        "custom" => &["type", "curve"],
        other => return Err(invalid(path, &format!("invalid dimming curve `{other}`"))),
    };
    require_allowed_mapping_keys(path, value, fields, "dimming curve")?;
    Ok(match string_field(path, value, "type")? {
        "linear" => DimmingCurve::Linear,
        "gamma" => DimmingCurve::Gamma(f32_field(path, value, "value")?),
        "custom" => DimmingCurve::Custom(parse_curve(path, required_field(path, value, "curve")?)?),
        other => return Err(invalid(path, &format!("invalid dimming curve `{other}`"))),
    })
}

fn parse_fixture_entries(
    path: &Utf8Path,
    value: &Value,
) -> Result<Vec<FixtureIndexedEntry>, LoadProjectError> {
    sequence_values(path, value, "entries")?
        .iter()
        .map(|entry| {
            require_allowed_mapping_keys(
                path,
                entry,
                &[
                    "id",
                    "name",
                    "dmx_min",
                    "dmx_max",
                    "curve_control",
                    "color",
                    "tag",
                ],
                "fixture indexed entry",
            )?;
            Ok(FixtureIndexedEntry {
                id: FixtureEntryId(u32_field(path, entry, "id")?),
                name: string_field(path, entry, "name")?.to_string(),
                dmx_min: u16::try_from(u32_field(path, entry, "dmx_min")?)
                    .map_err(|_| invalid(path, "dmx_min must be a u16"))?,
                dmx_max: u16::try_from(u32_field(path, entry, "dmx_max")?)
                    .map_err(|_| invalid(path, "dmx_max must be a u16"))?,
                curve_control: bool_field(path, entry, "curve_control")?,
                color: optional_string_field(entry, "color")
                    .map(parse_color)
                    .transpose()?,
                tag: optional_string_field(entry, "tag")
                    .map(parse_entry_tag)
                    .transpose()
                    .map_err(|message| invalid(path, &message))?,
            })
        })
        .collect()
}

fn parse_function_tag(value: &str) -> Result<FixtureFunctionTag, String> {
    Ok(match value {
        "pan" => FixtureFunctionTag::Pan,
        "tilt" => FixtureFunctionTag::Tilt,
        "dimmer" => FixtureFunctionTag::Dimmer,
        "shutter" => FixtureFunctionTag::Shutter,
        "zoom" => FixtureFunctionTag::Zoom,
        "gobo" => FixtureFunctionTag::Gobo,
        "frost" => FixtureFunctionTag::Frost,
        "prism" => FixtureFunctionTag::Prism,
        "color_wheel" => FixtureFunctionTag::ColorWheel,
        "color_mixing" => FixtureFunctionTag::ColorMixing,
        other => return Err(format!("invalid fixture function tag `{other}`")),
    })
}

fn parse_entry_tag(value: &str) -> Result<FixtureEntryTag, String> {
    Ok(match value {
        "shutter_open" => FixtureEntryTag::ShutterOpen,
        "shutter_closed" => FixtureEntryTag::ShutterClosed,
        "strobe" => FixtureEntryTag::Strobe,
        "prism_open" => FixtureEntryTag::PrismOpen,
        "prism_closed" => FixtureEntryTag::PrismClosed,
        "gobo_open" => FixtureEntryTag::GoboOpen,
        other => return Err(format!("invalid fixture entry tag `{other}`")),
    })
}

fn parse_color_component(value: &str) -> Option<ColorComponent> {
    match value {
        "red" => Some(ColorComponent::Red),
        "green" => Some(ColorComponent::Green),
        "blue" => Some(ColorComponent::Blue),
        "white" => Some(ColorComponent::White),
        _ => None,
    }
}
use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::control::{ControlClip, ControlClipId, ControlTarget, ControlValue};
use dawn_language::controller::{
    ArtNetConfig, ArtNetMode, Controller, ControllerId, ControllerPort, ControllerPortAddress,
    ControllerPortId, ControllerProtocol, E131Config, E131Mode,
};
use dawn_language::dsl::Identifier;
use dawn_language::effect::{
    CurveId, CurveSource, EffectDefinitionId, EffectInst, EffectInstId, EffectParamValue,
    EffectRef, GradientSource,
};
use dawn_language::element::{
    ColorCapability, DiscreteColorMapping, DiscreteEmitter, ElementCellAddress, ElementCellRange,
    ElementNode, ElementNodeId, ElementNodeKind, ElementSelection, ElementTree, ElementTreeId,
    EmitterId, IndexedOption, IndexedOptionId,
};
use dawn_language::fixture_profile::{
    ColorComponent, ColorMixingModel, ColorWheelColorMapping, DimmingCurve, FixtureBehaviorRule,
    FixtureChannel, FixtureChannelRole, FixtureEntryId, FixtureEntryTag, FixtureFunction,
    FixtureFunctionId, FixtureFunctionKind, FixtureFunctionTag, FixtureIndexedEntry,
    FixtureProfile, FixtureProfileId,
};
use dawn_language::model::DawnProject;
use dawn_language::operator::{
    BuiltinOperator, GraphOperatorNode, OperatorRef, validate_composition_graph,
};
use dawn_language::patch::{
    ByteOrder, FilterDefinition, PatchEdge, PatchGraph, PatchId, PatchNode, PatchNodeId,
    PatchPortId, PatchSink, PatchSource, PatchValueType,
};
use dawn_language::preview::{PreviewLayout, PreviewLayoutId, PropInstance, PropInstanceId};
use dawn_language::sequence::{
    AssetId, AutomationClip, AutomationClipId, CompositionGraphNode, CompositionGraphNodeId,
    CompositionGraphNodeKind, MarkCollectionKey, Sequence, SequenceAudio, SequenceCompositionGraph,
    SequenceId, SequenceLayerId,
};
use dawn_language::setup::{Setup, SetupId};
use indexmap::{IndexMap, IndexSet};
use yaml_serde::Value;

use super::Loader;
use super::parse::{
    ResolvedObject, bool_field, f32_field, i32_field, optional_field, optional_mapping,
    optional_sequence, optional_string_field, parse_automation_binding, parse_automation_curve,
    parse_color, parse_curve, parse_detached_automation_binding, parse_duration,
    parse_duration_as_time, parse_effect_scope, parse_gradient, parse_graph_edge,
    parse_graph_position, parse_mark_collection, parse_point3, parse_rotation3, parse_scale3,
    parse_sequence_layer, require_allowed_mapping_keys, required_field, sequence_field,
    sequence_values, string_field, u32_field, usize_field,
};
use crate::LoadProjectError;
use crate::diagnostics::{
    source_range_for_field_value, source_range_for_scalar, with_yaml_location,
};
use crate::source::ReferencedAsset;
