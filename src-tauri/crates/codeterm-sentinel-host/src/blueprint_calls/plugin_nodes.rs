use codeterm_blueprint_types::blueprint_document::{calls::FrozenClosure, NodeKind};
use codeterm_plugin::graph_nodes::GraphNodeRegistry;
use serde_json::{json, Map, Value};

/// The installed descriptor of every plugin step the closure uses, keyed by its `uses`.
/// A step whose plugin is removed or disabled is listed with `available: false`.
pub fn collect(
    registry: &GraphNodeRegistry,
    closure: &FrozenClosure,
    into: &mut Map<String, Value>,
) {
    for (id, descriptor) in descriptors(registry, closure) {
        into.entry(id).or_insert_with(|| json!(descriptor));
    }
}

pub fn descriptors(
    registry: &GraphNodeRegistry,
    closure: &FrozenClosure,
) -> std::collections::BTreeMap<String, codeterm_sentinel::blueprint_snapshot::PluginNode> {
    use codeterm_sentinel::blueprint_snapshot::{AvailablePluginNode, PluginNode};
    let mut nodes = std::collections::BTreeMap::new();
    let documents = std::iter::once(&closure.root.document)
        .chain(closure.callees.values().map(|callee| &*callee.document));
    for node in documents.flat_map(|document| document.nodes()) {
        let Some(NodeKind::Plugin(id)) = &node.uses else {
            continue;
        };
        nodes
            .entry(id.clone())
            .or_insert_with(|| match registry.step(id) {
                Some(step) => {
                    let descriptor = &step.descriptor;
                    PluginNode::Available(AvailablePluginNode {
                        available: true,
                        title: descriptor.title.clone(),
                        plugin: step.plugin.clone(),
                        description: descriptor.description.clone(),
                        inputs: descriptor.inputs.clone(),
                        outputs: descriptor.outputs.clone(),
                        outcomes: descriptor.outcomes.clone(),
                    })
                }
                None => PluginNode::Unavailable { available: false },
            });
    }
    nodes
}

#[cfg(test)]
mod tests;
