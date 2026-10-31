//! `SystemConfig.xml`: the element tree, the controllers and their outputs,
//! the previews, the output filters, and the patches between them. Vixen:
//! `Vixen.Core/IO/Xml` serializers, version 16.

use crate::xml::Element;
use std::collections::{HashMap, HashSet};

/// A node of the element tree. Vixen: `ElementNode`.
pub(crate) struct Node {
    pub(crate) name: String,
    /// The element whose channel a leaf is; a group has none.
    pub(crate) channel: Option<String>,
    /// Its children's ids, in order.
    pub(crate) children: Vec<String>,
}

pub(crate) struct ConfigController {
    pub(crate) name: String,
    /// The output module, by its type id.
    pub(crate) hardware_id: String,
    /// The module's settings in the module store, by instance id.
    pub(crate) hardware_instance_id: String,
    pub(crate) id: String,
    /// Each output's id, by output index.
    pub(crate) outputs: Vec<String>,
}

pub(crate) struct ConfigPreview {
    pub(crate) name: String,
    pub(crate) instance_id: String,
}

/// Component `component` takes its input from output `output` of `source`.
/// Vixen: `DataFlowPatch`.
pub(crate) struct Patch {
    pub(crate) component: String,
    pub(crate) source: String,
    pub(crate) output: u32,
}

#[derive(Default)]
pub(crate) struct Config {
    /// By node id. Vixen: `XmlElementNodeSerializer`, which keeps the first
    /// copy of a node written under several parents.
    pub(crate) nodes: HashMap<String, Node>,
    /// Node ids in the order the file first names them.
    pub(crate) node_order: Vec<String>,
    /// The tree's top nodes, in order.
    pub(crate) roots: Vec<String>,
    pub(crate) controllers: Vec<ConfigController>,
    pub(crate) previews: Vec<ConfigPreview>,
    /// Each output filter's type id, by instance id.
    pub(crate) filters: HashMap<String, String>,
    pub(crate) patches: Vec<Patch>,
    /// Disabled controllers and previews, by id.
    pub(crate) disabled: HashSet<String>,
}

fn id(element: &Element, name: &str) -> String {
    element
        .attribute(name)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn children<'a>(
    root: &'a Element,
    section: &'a str,
    item: &'a str,
) -> impl Iterator<Item = &'a Element> {
    root.child(section)
        .into_iter()
        .flat_map(move |section| section.children(item))
}

impl Config {
    pub(crate) fn read(root: &Element) -> Result<Self, String> {
        let mut config = Self::default();
        for node in children(root, "Nodes", "Node") {
            config.roots.push(id(node, "id"));
            config.node(node);
        }
        config.controllers = children(root, "Controllers", "Controller")
            .map(|controller| ConfigController {
                name: controller.attribute("name").unwrap_or_default().to_string(),
                hardware_id: id(controller, "hardwareId"),
                hardware_instance_id: id(controller, "hardwareInstanceId"),
                id: id(controller, "id"),
                outputs: controller
                    .child("Outputs")
                    .into_iter()
                    .flat_map(|outputs| outputs.children("Output"))
                    .map(|output| id(output, "id"))
                    .collect(),
            })
            .collect();
        config.previews = children(root, "Previews", "Preview")
            .map(|preview| ConfigPreview {
                name: preview.attribute("name").unwrap_or_default().to_string(),
                instance_id: id(preview, "instanceId"),
            })
            .collect();
        config.filters = children(root, "Filters", "FilterNode")
            .map(|filter| (id(filter, "instanceId"), id(filter, "typeId")))
            .collect();
        config.patches = children(root, "Patches", "Patch")
            .map(|patch| {
                let output = patch.attribute("output").unwrap_or_default();
                Ok(Patch {
                    component: id(patch, "componentId"),
                    source: id(patch, "sourceId"),
                    output: output
                        .trim()
                        .parse()
                        .map_err(|_| format!("a patch's output is `{output}`"))?,
                })
            })
            .collect::<Result<_, String>>()?;
        config.disabled = children(root, "DisabledDevices", "Device")
            .map(|device| id(device, "id"))
            .collect();
        Ok(config)
    }

    /// Reads a node and the nodes under it, keeping each node's first copy.
    fn node(&mut self, element: &Element) {
        let node_id = id(element, "id");
        if self.nodes.contains_key(&node_id) {
            return;
        }
        let nodes: Vec<&Element> = element.children("Node").collect();
        self.node_order.push(node_id.clone());
        self.nodes.insert(
            node_id,
            Node {
                name: element.attribute("name").unwrap_or_default().to_string(),
                channel: element
                    .attribute("channelId")
                    .map(|channel| channel.trim().to_ascii_lowercase()),
                children: nodes.iter().map(|node| id(node, "id")).collect(),
            },
        );
        for node in nodes {
            self.node(node);
        }
    }
}
