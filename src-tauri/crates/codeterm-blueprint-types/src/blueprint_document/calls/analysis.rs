use super::*;
use crate::blueprint_document::{Node, Port, ValueForm};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VerifiedCall {
    pub fixed_arguments: BTreeMap<String, String>,
    pub result_tainted: bool,
    pub expanded_steps: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallFinding {
    pub rule: String,
    pub name: String,
    pub field: String,
    pub location: Location,
    pub message: String,
    pub pin: Option<CallPin>,
}
impl CallFinding {
    pub fn into_diagnostic(self) -> Diagnostic {
        use crate::blueprint_document::checker::{Finding, Proof, ProofAtom};
        let arguments = self
            .pin
            .as_ref()
            .map(|pin| vec![pin.id.clone(), pin.hash.clone()])
            .unwrap_or_default();
        let atom = ProofAtom::new(&self.name, arguments, true, &self.field, self.location);
        Diagnostic::from_proof(
            Finding {
                rule: &self.rule,
                name: &self.name,
                severity: "error",
                message: &self.message,
                hint: "Correct the call contract; do not widen caller authority",
            },
            Proof {
                atom: atom.clone(),
                rule: self.rule.clone(),
                children: Vec::new(),
            },
            vec![atom],
            Vec::new(),
        )
    }
    fn new(
        parsed: &Parsed,
        node: &Node,
        field: &str,
        rule: &str,
        name: &str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            rule: rule.into(),
            name: name.into(),
            field: field.into(),
            location: parsed.at(field),
            message: message.into(),
            pin: node.call.as_ref().and_then(|call| call.pin.clone()),
        }
    }
}
pub fn check_facts(closure: &FrozenClosure) -> (BTreeMap<String, VerifiedCall>, Vec<CallFinding>) {
    let mut findings = Vec::new();
    let calls = graph(
        closure,
        &closure.root,
        &BTreeSet::new(),
        &mut Vec::new(),
        &mut findings,
    );
    (calls, findings)
}
fn graph(
    closure: &FrozenClosure,
    parsed: &Parsed,
    tainted_inputs: &BTreeSet<String>,
    stack: &mut Vec<CallPin>,
    findings: &mut Vec<CallFinding>,
) -> BTreeMap<String, VerifiedCall> {
    let mut verified = BTreeMap::new();
    for (index, node) in parsed.document.nodes().enumerate() {
        let Some(call) = &node.call else { continue };
        let path = if index < parsed.document.steps.len() {
            format!("steps[{index}].call")
        } else {
            format!("other[{}].call", index - parsed.document.steps.len())
        };
        let start = findings.len();
        let mut fail = |field: &str, rule: &str, name: &str, message: String| {
            findings.push(CallFinding::new(parsed, node, field, rule, name, message))
        };
        let Some(pin) = call.pin.as_ref() else {
            fail(
                &path,
                "C1",
                "unresolved_call",
                "Call has no verified id/hash".into(),
            );
            continue;
        };
        let callee = match closure.callee(pin) {
            Ok(callee) => callee,
            Err(error) => {
                fail(&path, "C1", "unresolved_call", error);
                continue;
            }
        };
        if !callee.document.other.is_empty() {
            fail(
                &path,
                "C11",
                "callable_other",
                "callee other handlers are not supported inside call; move them to the caller"
                    .into(),
            );
        }
        if stack.contains(pin) {
            fail(
                &path,
                "C7",
                "recursive_call",
                "Recursive call is forbidden".into(),
            );
            continue;
        }
        if stack.len() >= MAX_CALL_DEPTH {
            fail(
                &path,
                "C12",
                "call_depth",
                "budget_exhausted: call depth exceeds eight".into(),
            );
            continue;
        }
        for (name, input) in &callee.document.inputs {
            let field = format!("{path}.with.{name}");
            let Some(argument) = call.arguments.get(name) else {
                if input.default.is_none() {
                    fail(
                        &field,
                        "C2",
                        "missing_argument",
                        format!("Missing argument {name}"),
                    );
                }
                continue;
            };
            let expected = ports::expand(input, &callee.document.types);
            let actual = ports::form(&parsed.document, argument, index);
            if !actual
                .as_ref()
                .zip(expected.as_ref())
                .is_some_and(|(a, e)| ports::compatible(a, e))
            {
                fail(&field,"C4","argument_type",format!("Argument {name} does not satisfy the callee schema, including selected fields"));
            }
        }
        for name in call
            .arguments
            .keys()
            .filter(|name| !callee.document.inputs.contains_key(*name))
        {
            fail(
                &format!("{path}.with.{name}"),
                "C3",
                "extra_argument",
                format!("Unknown callee argument {name}"),
            );
        }
        if let Some(expected) = callee
            .document
            .steps
            .first()
            .and_then(|n| n.input.as_ref())
            .and_then(|p| ports::expand(p, &callee.document.types))
        {
            let actual = callee_input(&callee.document);
            if !actual.is_some_and(|actual| ports::compatible(&actual, &expected)) {
                fail(
                    &path,
                    "C5",
                    "callee_input_port",
                    "Callee inputs do not satisfy its first input port".into(),
                );
            }
            if node.input.as_ref().is_some_and(|actual| {
                ports::expand(actual, &parsed.document.types)
                    .is_none_or(|actual| !ports::compatible(&actual, &expected))
            }) {
                fail(
                    &path,
                    "C5",
                    "callee_input_port",
                    "Call input port differs from the callee input port".into(),
                );
            }
            if let Some(binding) = node.bind.get("in") {
                if !ports::binding(&parsed.document, binding, index)
                    .is_some_and(|actual| ports::compatible(&actual, &expected))
                {
                    fail(
                        &path,
                        "C5",
                        "callee_input_field",
                        "Bound input field does not satisfy the callee input port".into(),
                    );
                }
            }
        }
        let result = callee
            .document
            .result
            .port
            .as_ref()
            .and_then(|p| ports::expand(p, &callee.document.types));
        let from = callee
            .document
            .result
            .from
            .as_ref()
            .and_then(|b| ports::binding(&callee.document, b, callee.document.nodes().count()));
        if !from
            .as_ref()
            .zip(result.as_ref())
            .is_some_and(|(a, e)| ports::compatible(a, e))
        {
            fail(
                &path,
                "C6",
                "callee_result",
                "Callee result source does not satisfy its declared result port".into(),
            );
        }
        if node.output.as_ref().is_some_and(|expected| {
            !ports::expand(expected, &parsed.document.types)
                .as_ref()
                .zip(result.as_ref())
                .is_some_and(|(e, a)| ports::compatible(a, e))
        }) {
            fail(
                &path,
                "C6",
                "call_result",
                "Call output differs from callee result port".into(),
            );
        }
        let mut tainted = BTreeSet::new();
        let mut fixed = BTreeMap::new();
        for (name, arg) in &call.arguments {
            if is_tainted(arg, tainted_inputs, &verified) {
                tainted.insert(name.clone());
            } else {
                fixed.insert(name.clone(), serde_json::to_string(arg).unwrap_or_default());
            }
        }
        for (child_index, child) in callee.document.nodes().enumerate() {
            if matches!(child.kind(), "blueprint" | "stop" | "respond_prompt") {
                fail(&path,"C11","scheduler_custody",format!("callee effect {} requires scheduler custody and is not supported inside call; move it to the caller (callee node {})",child.kind(),child.id));
            }
            if child
                .caps
                .iter()
                .chain(child.code.iter().flat_map(|code| &code.caps))
                .any(|cap| !node.caps.contains(cap) || cap == "domios-cli")
            {
                fail(
                    &path,
                    "C9",
                    "callee_grant_widening",
                    format!("Callee node {} exceeds caller call grants", child.id),
                );
            }
            for (slot, value) in executable_values(child) {
                if is_tainted(value, &tainted, &BTreeMap::new()) {
                    fail(&path,"C8","boundary_taint",format!("Tainted argument reaches callee node {} executable slot {slot} (node {child_index})",child.id));
                }
            }
        }
        stack.push(pin.clone());
        let children = graph(closure, &callee.parsed(), &tainted, stack, findings);
        stack.pop();
        if findings.len() == start {
            let result_tainted = callee.document.result.from.as_ref().is_none_or(|binding| {
                let id = binding.split('.').next().unwrap_or_default();
                children.get(id).is_none_or(|c| c.result_tainted)
            });
            verified.insert(
                node.id.clone(),
                VerifiedCall {
                    fixed_arguments: fixed,
                    result_tainted,
                    expanded_steps: callee.document.nodes().count()
                        + children.values().map(|c| c.expanded_steps).sum::<usize>(),
                },
            );
        }
    }
    verified
}
fn callee_input(document: &Document) -> Option<Port> {
    if document.inputs.len() == 1 {
        return ports::expand(document.inputs.values().next()?, &document.types);
    }
    Some(Port {
        kind: "object".into(),
        properties: document
            .inputs
            .iter()
            .map(|(k, v)| Some((k.clone(), ports::expand(v, &document.types)?)))
            .collect::<Option<_>>()?,
        required: document
            .inputs
            .iter()
            .filter(|(_, v)| v.default.is_none())
            .map(|(k, _)| k.clone())
            .collect(),
        additional_properties: Some(false),
        ..Default::default()
    })
}
fn is_tainted(
    value: &ValueForm,
    tainted: &BTreeSet<String>,
    calls: &BTreeMap<String, VerifiedCall>,
) -> bool {
    match value {
        ValueForm::Lit { .. } => false,
        ValueForm::Input { input } => tainted.contains(input),
        ValueForm::Bind { bind } => calls
            .get(bind.split('.').next().unwrap_or_default())
            .is_none_or(|c| c.result_tainted),
    }
}
fn executable_values(node: &Node) -> Vec<(&str, &ValueForm)> {
    let mut values = Vec::new();
    if let Some(run) = &node.run {
        values.push(("run", run));
    }
    for (key, value) in &node.params {
        if matches!(
            key.as_str(),
            "command" | "target" | "url" | "task" | "file" | "source" | "provider" | "profile"
        ) {
            values.push((key.as_str(), value));
        }
    }
    values
}

pub fn arguments(
    call: &super::super::Call,
    inputs: &BTreeMap<String, Value>,
    outputs: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>, String> {
    call.arguments
        .iter()
        .map(|(key, form)| {
            let value = match form {
                ValueForm::Lit { lit } => lit.clone(),
                ValueForm::Input { input } => {
                    inputs.get(input).cloned().ok_or("Missing caller input")?
                }
                ValueForm::Bind { bind } => {
                    let mut parts = bind.split('.');
                    let id = parts.next().ok_or("Missing bound node")?;
                    if parts.next() != Some("out") {
                        return Err("Bind a node output".into());
                    }
                    let mut value = outputs.get(id).ok_or("Missing bound output")?;
                    for field in parts {
                        value = value.get(field).ok_or("Missing bound field")?;
                    }
                    value.clone()
                }
            };
            Ok((key.clone(), value))
        })
        .collect()
}
