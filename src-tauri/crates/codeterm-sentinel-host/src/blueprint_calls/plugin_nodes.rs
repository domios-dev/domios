use codeterm_blueprint_types::blueprint_document::{calls::FrozenClosure, NodeKind};
use codeterm_plugin::graph_nodes::GraphNodeRegistry;
use serde_json::{json, Map, Value};

/// The installed descriptor of every plugin step the closure uses, keyed by its `uses`.
/// A step whose plugin is removed or disabled is listed with `available: false`.
pub fn collect(registry: &GraphNodeRegistry, closure: &FrozenClosure, into: &mut Map<String, Value>) {
    let documents = std::iter::once(&closure.root.document)
        .chain(closure.callees.values().map(|callee| &*callee.document));
    for node in documents.flat_map(|document| document.nodes()) {
        let Some(NodeKind::Plugin(id)) = &node.uses else {
            continue;
        };
        into.entry(id.clone()).or_insert_with(|| match registry.step(id) {
            Some(step) => {
                let descriptor = &step.descriptor;
                json!({"title":descriptor.title,"plugin":step.plugin,"description":descriptor.description,
                    "inputs":descriptor.inputs,"outputs":descriptor.outputs,"outcomes":descriptor.outcomes,"available":true})
            }
            None => json!({"available":false}),
        });
    }
}

pub fn descriptors(registry: &GraphNodeRegistry, closure: &FrozenClosure) -> Value {
    let mut nodes = Map::new();
    collect(registry, closure, &mut nodes);
    Value::Object(nodes)
}

#[cfg(test)]
mod tests;
