use super::*;
use std::collections::BTreeSet;
type Errors = Vec<(String, String)>;
pub(super) fn document(doc: &Document) -> Errors {
    let mut errors = Vec::new();
    if doc.schema_version != SCHEMA_VERSION {
        errors.push((
            "schema_version".into(),
            "Only schema_version: 2 is supported".into(),
        ));
    }
    if let Some(key) = whole_input(&doc.name) {
        if key.is_empty() {
            errors.push(("name".into(), "Name input must be nonempty".into()));
        }
    } else if let Err(e) = crate::automation_name::validate_sentinel_name(&doc.name) {
        errors.push(("name".into(), e));
    }
    if doc.name.contains("{{") && whole_input(&doc.name).is_none() {
        errors.push((
            "name".into(),
            "Inputs occupy a whole value; name templates are unsupported".into(),
        ));
    }
    if let Err(e) = crate::automation_name::validate_note(doc.note.as_deref()) {
        errors.push(("note".into(), e));
    }
    if doc
        .owner
        .as_deref()
        .is_some_and(|s| !matches!(s, "tab" | "team"))
    {
        errors.push(("owner".into(), "Use tab or team".into()));
    }
    if let Some(target) = doc.result.to.as_ref().and_then(ResultTarget::fixed) {
        if serde_json::from_value::<crate::blueprint_pipeline::ResultTarget>(target.into()).is_err()
        {
            errors.push((
                "result.to".into(),
                format!("Unknown result target `{target}`; use owner, team, notify or tab:<id>"),
            ));
        }
    }
    if doc.kind == Kind::Pipeline
        && doc.trigger.is_none()
        && (doc.result.port.is_none() || doc.result.from.is_none())
    {
        errors.push((
            "trigger".into(),
            "A pipeline requires a trigger or a typed callable result".into(),
        ));
    }
    if doc.kind == Kind::Watch && doc.trigger.is_some() {
        errors.push((
            "trigger".into(),
            "Watch timing belongs on its source node".into(),
        ));
    }
    if serde_json::to_vec(doc).map_or(true, |bytes| bytes.len() > MAX_RECIPE_BYTES) {
        errors.push((String::new(), "Blueprint document exceeds 256 KiB".into()));
    }
    if let Some(Trigger::Webhook {
        schema: Some(schema),
    }) = &doc.trigger
    {
        port(schema, "trigger.schema", &doc.types, 0, &mut errors);
    }
    ports(&doc.inputs, "inputs", &doc.types, &mut errors);
    ports(&doc.types, "types", &doc.types, &mut errors);
    nodes(
        &doc.steps,
        &doc.other,
        &doc.inputs,
        &doc.types,
        "",
        &mut errors,
    );
    for (name, sub) in &doc.blueprints {
        let path = format!("blueprints.{name}");
        ports(
            &sub.inputs,
            &format!("{path}.inputs"),
            &sub.types,
            &mut errors,
        );
        ports(
            &sub.types,
            &format!("{path}.types"),
            &sub.types,
            &mut errors,
        );
        nodes(
            &sub.steps,
            &sub.other,
            &sub.inputs,
            &sub.types,
            &format!("{path}."),
            &mut errors,
        );
    }
    errors
}
fn ports(
    ps: &BTreeMap<String, Port>,
    path: &str,
    types: &BTreeMap<String, Port>,
    errors: &mut Errors,
) {
    for (k, p) in ps {
        port(p, &format!("{path}.{k}"), types, 0, errors);
    }
}
fn port(p: &Port, path: &str, types: &BTreeMap<String, Port>, depth: usize, errors: &mut Errors) {
    if depth > 32 {
        errors.push((path.into(), "Port schema exceeds depth 32".into()));
        return;
    }
    if !matches!(
        p.kind.as_str(),
        "" | "string" | "integer" | "number" | "boolean" | "object" | "array" | "null"
    ) {
        errors.push((path.into(), "Unsupported JSON Schema type".into()));
    }
    if let Some(reference) = &p.reference {
        if !types.contains_key(reference) {
            errors.push((path.into(), format!("Unknown port type {reference}")));
        }
        if !p.kind.is_empty() || !p.properties.is_empty() {
            errors.push((
                path.into(),
                "A $ref cannot be combined with an inline type".into(),
            ));
        }
    }
    if p.reference.is_none() && p.kind.is_empty() && p.choices.is_none() {
        errors.push((path.into(), "Declare a type, enum or $ref".into()));
    }
    if p.default
        .as_ref()
        .is_some_and(|v| !p.accepts_with_types(v, types))
    {
        errors.push((
            format!("{path}.default"),
            "Default violates input schema".into(),
        ));
    }
    for (k, child) in &p.properties {
        port(
            child,
            &format!("{path}.properties.{k}"),
            types,
            depth + 1,
            errors,
        );
    }
    if let Some(items) = &p.items {
        port(items, &format!("{path}.items"), types, depth + 1, errors);
    }
    if p.required.iter().any(|k| !p.properties.contains_key(k)) {
        errors.push((path.into(), "Required property is not declared".into()));
    }
}
fn nodes(
    steps: &[Node],
    other: &[Node],
    inputs: &BTreeMap<String, Input>,
    types: &BTreeMap<String, Port>,
    prefix: &str,
    errors: &mut Errors,
) {
    let all = steps.iter().chain(other).collect::<Vec<_>>();
    if steps.is_empty() {
        errors.push((format!("{prefix}steps"), "Declare at least one node".into()));
    }
    if all.len() > 64 {
        errors.push((
            format!("{prefix}steps"),
            "Blueprint exceeds 64 nodes".into(),
        ));
    }
    let ids = all
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    for (i, node) in all.iter().enumerate() {
        let path = if i < steps.len() {
            format!("{prefix}steps[{i}]")
        } else {
            format!("{prefix}other[{}]", i - steps.len())
        };
        if node.id.is_empty()
            || node.id.len() > 64
            || !node
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            || matches!(node.id.as_str(), "next" | "stop")
            || !seen.insert(&node.id)
        {
            errors.push((
                format!("{path}.id"),
                "Use a unique bounded node id; next and stop are reserved".into(),
            ));
        }
        if usize::from(node.uses.is_some())
            + usize::from(node.run.is_some())
            + usize::from(node.code.is_some())
            + usize::from(node.call.is_some())
            != 1
        {
            errors.push((
                path.clone(),
                "Choose exactly one uses, run, code or call".into(),
            ));
        }
        errors.extend(super::code_contract::errors(node, &path));
        if let Some(code) = &node.code {
            if code.src.is_some() == code.file.is_some() {
                errors.push((
                    format!("{path}.code"),
                    "Choose exactly one src or file".into(),
                ));
            }
            if code.file.as_ref().is_some_and(|p| {
                std::path::Path::new(p).is_absolute() || p.split(['/', '\\']).any(|c| c == "..")
            }) {
                errors.push((
                    format!("{path}.code.file"),
                    "Use a local relative file without parent traversal".into(),
                ));
            }
        }
        if let Some(call) = &node.call {
            if call.reference.is_empty() {
                errors.push((
                    format!("{path}.call.ref"),
                    "A call needs a local or inline reference".into(),
                ));
            }
        }
        if node.buckets.len() > 32 || node.outcomes.len() > 32 || node.routes.len() > 32 {
            errors.push((
                path.clone(),
                "Node exceeds the 32 bucket/outcome/route bound".into(),
            ));
        }
        if node
            .caps
            .iter()
            .chain(node.code.iter().flat_map(|c| &c.caps))
            .any(|cap| cap == "domios-cli")
        {
            errors.push((
                format!("{path}.caps"),
                "domios-cli capability is not approved".into(),
            ));
        }
        for (outcome, target) in &node.routes {
            if !matches!(target.as_str(), "next" | "@next" | "stop" | "@stop")
                && !ids.get(target.as_str()).is_some_and(|target| *target > i)
            {
                errors.push((
                    format!("{path}.routes.{outcome}"),
                    "Routes must target a known forward node or @next/@stop".into(),
                ));
            }
        }
        let mut buckets = BTreeSet::new();
        if node
            .buckets
            .iter()
            .any(|b| b.is_empty() || b.len() > 128 || b == "other" || !buckets.insert(b))
        {
            errors.push((
                format!("{path}.buckets"),
                "Declare unique bounded buckets; other is reserved".into(),
            ));
        }
        for (field, value) in node
            .params
            .iter()
            .map(|(k, v)| (format!("with.{k}"), v))
            .chain(
                node.matches
                    .iter()
                    .map(|(k, v)| (format!("matches.{k}"), v)),
            )
            .chain(node.run.iter().map(|v| ("run".into(), v)))
            .chain(node.every.iter().map(|v| ("every".into(), v)))
            .chain(node.timeout.iter().map(|v| ("timeout".into(), v)))
            .chain(node.call.iter().flat_map(|c| {
                c.arguments
                    .iter()
                    .map(|(k, v)| (format!("call.with.{k}"), v))
            }))
        {
            match value {
                ValueForm::Bind { bind } => {
                    if !data_slot(node, &field) {
                        errors.push((
                            format!("{path}.{field}"),
                            "Runtime binds are allowed only in data slots".into(),
                        ));
                    }
                    binding(bind, &ids, i, &format!("{path}.{field}"), errors);
                }
                ValueForm::Input { input } if !inputs.contains_key(input) => {
                    errors.push((
                        format!("{path}.{field}"),
                        format!("Undeclared input {input}"),
                    ));
                }
                _ => {}
            }
        }
        for (field, bind) in &node.bind {
            if !data_slot(node, field) && !matches!(node.kind(), "code" | "command") {
                errors.push((
                    format!("{path}.bind.{field}"),
                    "Runtime binds are allowed only in data slots".into(),
                ));
            }
            binding(bind, &ids, i, &format!("{path}.bind.{field}"), errors);
        }
        if node.bind.contains_key("in") && node.bind.len() > 1 {
            errors.push((
                format!("{path}.bind"),
                "Use bind.in alone or named input ports, never both".into(),
            ));
        }
        if node.bind.len() > 32
            || matches!(node.kind(), "code" | "command")
                && node
                    .bind
                    .keys()
                    .any(|key| key.is_empty() || key.len() > 64 || key.contains('.'))
        {
            errors.push((
                format!("{path}.bind"),
                "Use at most 32 nonempty input port names of at most 64 bytes without dots".into(),
            ));
        }
        for (field, p) in [("in", &node.input), ("out", &node.output)] {
            if let Some(p) = p {
                port(p, &format!("{path}.{field}"), types, 0, errors);
            }
        }
    }
}
fn binding(bind: &str, ids: &BTreeMap<&str, usize>, order: usize, path: &str, errors: &mut Errors) {
    if !bindings::parse(bind).is_ok_and(|binding| ids.get(binding.node).is_some_and(|i| *i < order))
    {
        errors.push((
            path.into(),
            "Bind a preceding node's out port, optionally a field".into(),
        ));
    }
}

fn data_slot(node: &Node, slot: &str) -> bool {
    matches!(
        (node.kind(), slot),
        ("notify", "with.text")
            | ("send", "with.payload")
            | ("command" | "code" | "ask" | "agent" | "call", "in")
    ) || node.call.is_some() && slot.starts_with("call.with.")
}
