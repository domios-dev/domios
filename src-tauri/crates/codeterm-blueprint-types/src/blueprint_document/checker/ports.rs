use super::super::Port;
use serde_json::Value;
use std::collections::BTreeMap;

const MAX_EXPANDED_NODES: usize = 4096;
const PAYLOAD_BYTES_PER_NODE: usize = 256;

#[derive(Clone)]
struct Resolved {
    port: Port,
    nodes: usize,
    depth: usize,
}

pub(super) struct Resolver<'a> {
    types: &'a BTreeMap<String, Port>,
    memo: BTreeMap<String, Resolved>,
    remaining: usize,
}
impl<'a> Resolver<'a> {
    pub(super) fn new(types: &'a BTreeMap<String, Port>) -> Self {
        Self {
            types,
            memo: BTreeMap::new(),
            remaining: MAX_EXPANDED_NODES,
        }
    }
    fn charge(&mut self, nodes: usize) -> Result<(), String> {
        self.remaining = self.remaining.checked_sub(nodes).ok_or_else(|| {
            format!("Port schema exceeds expanded-node budget {MAX_EXPANDED_NODES}")
        })?;
        Ok(())
    }
    pub(super) fn resolve(&mut self, port: &Port) -> Result<Port, String> {
        self.walk(port, &mut Vec::new(), 0).map(|r| r.port)
    }
    fn walk(
        &mut self,
        p: &Port,
        stack: &mut Vec<String>,
        depth: usize,
    ) -> Result<Resolved, String> {
        if depth > 32 {
            return Err("Port schema exceeds depth 32".into());
        }
        if let Some(name) = &p.reference {
            if stack.contains(name) {
                return Err(format!("Cyclic port type {name}"));
            }
            if let Some(cached) = self.memo.get(name) {
                if depth + 1 + cached.depth > 32 {
                    return Err("Port schema exceeds depth 32".into());
                }
                let nodes = cached.nodes;
                self.charge(nodes)?;
                let mut resolved = self.memo[name].clone();
                resolved.depth += 1;
                return Ok(resolved);
            }
            let target = self
                .types
                .get(name)
                .ok_or_else(|| format!("Unknown port type {name}"))?;
            stack.push(name.clone());
            let resolved = self.walk(target, stack, depth + 1);
            stack.pop();
            let mut resolved = resolved?;
            self.charge(resolved.nodes)?;
            self.memo.insert(name.clone(), resolved.clone());
            resolved.depth += 1;
            return Ok(resolved);
        }
        let mut header = p.clone();
        header.properties.clear();
        header.items = None;
        let payload_bytes = serde_json::to_vec(&header)
            .map_err(|error| error.to_string())?
            .len()
            + p.properties.keys().map(String::len).sum::<usize>();
        let nodes = 1 + payload_bytes / PAYLOAD_BYTES_PER_NODE;
        self.charge(nodes)?;
        let mut result = Resolved {
            port: header,
            nodes,
            depth: 0,
        };
        for (key, child) in &p.properties {
            let child = self.walk(child, stack, depth + 1)?;
            result.nodes += child.nodes;
            result.depth = result.depth.max(child.depth + 1);
            result.port.properties.insert(key.clone(), child.port);
        }
        if let Some(child) = &p.items {
            let child = self.walk(child, stack, depth + 1)?;
            result.nodes += child.nodes;
            result.depth = result.depth.max(child.depth + 1);
            result.port.items = Some(Box::new(child.port));
        }
        Ok(result)
    }
}

pub fn resolve(port: &Port, types: &BTreeMap<String, Port>) -> Result<Port, String> {
    Resolver::new(types).resolve(port)
}

pub(super) fn preflight(doc: &super::super::Document) -> Result<(), (String, String)> {
    let mut resolver = Resolver::new(&doc.types);
    let schemas = doc
        .types
        .iter()
        .map(|(name, p)| (format!("types.{name}"), p))
        .chain(
            doc.inputs
                .iter()
                .map(|(name, p)| (format!("inputs.{name}"), p)),
        )
        .chain(doc.nodes().enumerate().flat_map(|(i, node)| {
            let path = super::slots::node_path(doc, i);
            [("in", &node.input), ("out", &node.output)]
                .into_iter()
                .filter_map(move |(direction, p)| {
                    p.as_ref().map(|p| (format!("{path}.{direction}"), p))
                })
        }))
        .chain(doc.result.port.iter().map(|p| ("result.port".into(), p)));
    for (field, port) in schemas {
        resolver.resolve(port).map_err(|message| (field, message))?;
    }
    Ok(())
}

fn kind(port: &Port) -> &str {
    if !port.kind.is_empty() {
        return &port.kind;
    }
    if !port.properties.is_empty() || !port.required.is_empty() {
        return "object";
    }
    if port.items.is_some() {
        return "array";
    }
    if let Some(choices) = &port.choices {
        if let Some(first) = choices.first() {
            let value_kind = |v: &Value| match v {
                Value::String(_) => "string",
                Value::Bool(_) => "boolean",
                Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
                Value::Number(_) => "number",
                Value::Null => "null",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };
            let k = value_kind(first);
            if choices.iter().all(|v| value_kind(v) == k) {
                return k;
            }
        }
    }
    ""
}

pub fn subsumption_conflicts(source: &Port, target: &Port) -> Vec<String> {
    fn walk(source: &Port, target: &Port, path: &str, depth: usize, out: &mut Vec<String>) {
        let mut add = |reason: String| out.push(format!("{path}: {reason}"));
        if depth > 32 {
            add("schema comparison exceeds depth 32".into());
            return;
        }
        if source.reference.is_some() || target.reference.is_some() {
            add("resolve schema references before comparison".into());
            return;
        }
        if let Some(choices) = &source.choices {
            if choices.iter().all(|v| target.accepts(v))
                && (target.media.is_none() || source.media == target.media)
            {
                return;
            }
        }
        let (s, t) = (kind(source), kind(target));
        if !t.is_empty() && !(s == t || s == "integer" && t == "number") {
            add(format!(
                "{} is not {t}",
                if s.is_empty() { "untyped value" } else { s }
            ));
            return;
        }
        if target.choices.as_ref().is_some_and(|allowed| {
            source
                .choices
                .as_ref()
                .is_none_or(|values| values.iter().any(|v| !allowed.contains(v)))
        }) {
            add("source enum is wider than target enum".into());
        }
        if let Some(maximum) = target.max_length {
            if source.max_length.is_none_or(|size| size > maximum) {
                add(format!("source maxLength is wider than {maximum}"));
            }
        }
        if let Some(media) = &target.media {
            if source.media.as_ref() != Some(media) {
                add(format!("source media does not guarantee {media}"));
            }
        }
        if t == "object" || s == "object" {
            for name in &target.required {
                if !source.required.contains(name) {
                    add(format!("missing required field {name}"));
                }
            }
            if target.additional_properties == Some(false) {
                if source.additional_properties != Some(false) {
                    add("source permits extra fields in a closed target".into());
                }
                for name in source.properties.keys() {
                    if !target.properties.contains_key(name) {
                        add(format!("extra field {name} is not allowed"));
                    }
                }
            }
            for (name, wanted) in &target.properties {
                if let Some(actual) = source.properties.get(name) {
                    walk(actual, wanted, &format!("{path}.{name}"), depth + 1, out);
                } else if source.additional_properties != Some(false) {
                    out.push(format!(
                        "{path}.{name}: source permits an unconstrained property"
                    ));
                }
            }
        }
        if t == "array" || s == "array" {
            if let Some(wanted) = &target.items {
                walk(
                    source.items.as_deref().unwrap_or(&Port::default()),
                    wanted,
                    &format!("{path}[]"),
                    depth + 1,
                    out,
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(source, target, "$", 0, &mut out);
    out
}

pub(super) fn field<'a>(port: &'a Port, path: &str) -> Option<&'a Port> {
    if path.is_empty() {
        return Some(port);
    }
    path.split('.').try_fold(port, |p, part| {
        if p.required.iter().any(|key| key == part) {
            p.properties.get(part)
        } else {
            None
        }
    })
}
