use super::super::{
    projection::{self, SemanticDiff},
    Document, Kind, Node, NodeKind, Parsed, Port, Trigger, ValueForm,
};
use super::{slots, CheckContext, Diagnostic};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize)]
pub struct FixResult {
    pub changed: bool,
    pub text: Option<String>,
    pub diff: String,
    pub semantic_diff: Option<SemanticDiff>,
    pub notes: Vec<String>,
    pub refusals: Vec<String>,
}

pub fn fix(parsed: &Parsed, source: &str, context: &CheckContext) -> FixResult {
    fix_with_assignments(parsed, source, context, &[])
}
pub fn fix_with_assignments(
    parsed: &Parsed,
    source: &str,
    context: &CheckContext,
    assignments: &[String],
) -> FixResult {
    let before = &parsed.document;
    let mut result = FixResult {
        changed: false,
        text: None,
        diff: String::new(),
        semantic_diff: None,
        notes: Vec::new(),
        refusals: Vec::new(),
    };
    let diagnostics = super::check_ir(&parsed.ir(), context);
    let mut flat = Vec::new();
    flatten(&diagnostics, &mut flat);
    for diagnostic in &flat {
        if matches!(diagnostic.rule.as_str(), "R7" | "R8" | "R9" | "R10") {
            result.refusals.push(format!(
                "{} {}: executable and authority repairs require author/owner review",
                diagnostic.rule, diagnostic.field
            ));
        }
    }
    let mut missing: BTreeMap<String, Vec<Option<Port>>> = BTreeMap::new();
    for use_ in slots::input_uses(before) {
        if !before.inputs.contains_key(&use_.name) {
            missing.entry(use_.name).or_default().push(use_.expected);
        }
    }
    let mut declarations = BTreeMap::new();
    for (name, expected) in missing {
        let kinds = expected
            .iter()
            .filter_map(|p| p.as_ref().map(|p| p.kind.clone()))
            .collect::<BTreeSet<_>>();
        if expected.iter().any(Option::is_none) || kinds.len() != 1 || kinds.contains("") {
            result.refusals.push(format!(
                "Input {name} has no unambiguous type across every use"
            ));
        } else if let Some(kind) = kinds.into_iter().next() {
            declarations.insert(
                name,
                Port {
                    kind,
                    ..Default::default()
                },
            );
        }
    }
    if !result.refusals.is_empty() {
        return result;
    }
    let mut after = before.clone();
    for (name, port) in declarations {
        result
            .notes
            .push(format!("Declared inputs.{name} as {}", port.kind));
        after.inputs.insert(name, port);
    }
    if let Some(Trigger::Schedule { every }) = &mut after.trigger {
        normalize(every, "trigger.every", &mut result.notes);
    }
    for (i, node) in after.steps.iter_mut().chain(&mut after.other).enumerate() {
        let path = slots::node_path(before, i);
        for (slot, form) in [("every", &mut node.every), ("timeout", &mut node.timeout)] {
            if let Some(form) = form {
                normalize(form, &format!("{path}.{slot}"), &mut result.notes);
            }
        }
    }
    let unhandled = before
        .nodes()
        .filter(|n| n.kind() == "ask" && !n.buckets.is_empty() && !n.routes.contains_key("other"))
        .map(|n| n.id.clone())
        .collect::<Vec<_>>();
    if before.kind == Kind::Pipeline && !unhandled.is_empty() {
        result
            .refusals
            .push("Pipeline other escalation requires runtime support; repair refused".into());
        result.notes.clear();
        return result;
    }
    for id in unhandled {
        escalate(&mut after, &id, &mut result.notes);
    }
    if before.nodes().count() <= 64 && after.nodes().count() > 64 {
        result
            .refusals
            .push("Adding an escalation would exceed the 64-node bound".into());
        result.notes.clear();
        return result;
    }
    let text = match serde_yaml::to_string(&after) {
        Ok(text) => text,
        Err(error) => {
            result.refusals.push(error.to_string());
            return result;
        }
    };
    let checked =
        super::super::check_with_context(&parsed.at("").file, &text, assignments, context);
    if !checked.ok {
        result.refusals.push(format!(
            "Candidate is not admissible: {}",
            super::render(&checked.diagnostics, false)
        ));
        result.notes.clear();
        return result;
    }
    result.changed = source != text;
    if result.changed {
        let file = parsed.at("").file;
        result.diff = unified_diff(&file, source, &text);
        result.semantic_diff = Some(projection::diff(before, &after));
        if before == &after {
            result
                .notes
                .push("Canonicalized mapping keys and formatting".into());
        }
    }
    result.text = Some(text);
    result
}
fn normalize(form: &mut ValueForm, field: &str, notes: &mut Vec<String>) {
    if let Some(ms) = slots::duration_ms(form) {
        if ms < 10_000 {
            notes.push(format!(
                "WARNING: normalized {field} from {ms}ms to 10s; this changes timing"
            ));
            *form = ValueForm::literal("10s");
        }
    }
}
fn flatten<'a>(diagnostics: &'a [Diagnostic], flat: &mut Vec<&'a Diagnostic>) {
    for d in diagnostics {
        flat.push(d);
        flatten(&d.folded, flat);
    }
}
fn escalate(doc: &mut Document, decision: &str, notes: &mut Vec<String>) {
    let Some(index) = doc.nodes().position(|n| n.id == decision) else {
        return;
    };
    let existing = doc
        .nodes()
        .enumerate()
        .find(|(i, n)| {
            *i > index
                && n.kind() == "escalate"
                && n.params
                    .get("target")
                    .and_then(ValueForm::value)
                    .and_then(serde_json::Value::as_str)
                    == Some("creator")
                && n.routes.is_empty()
        })
        .map(|(_, n)| n.id.clone());
    let target = if let Some(id) = existing {
        id
    } else {
        let mut id = "escalate_other".to_string();
        while doc.nodes().any(|n| n.id == id) {
            id.push('_');
        }
        doc.other.push(Node {
            id: id.clone(),
            uses: Some(NodeKind::Escalate),
            params: BTreeMap::from([("target".into(), ValueForm::literal("creator"))]),
            ..Default::default()
        });
        id
    };
    let watch = doc.kind == Kind::Watch;
    if let Some(node) = doc
        .steps
        .iter_mut()
        .chain(&mut doc.other)
        .find(|n| n.id == decision)
    {
        node.routes.insert("other".into(), target.clone());
        if watch && node.escalation_timeout_ms.is_none() {
            node.escalation_timeout_ms = Some(60_000);
            notes.push(format!("Set {decision} creator escalation timeout to 60s"));
        }
        if !node.outcomes.is_empty() && !node.outcomes.iter().any(|o| o == "other") {
            node.outcomes.push("other".into());
        }
    }
    notes.push(format!(
        "Routed {decision}.other to {target} (creator escalation)"
    ));
}
fn unified_diff(file: &str, before: &str, after: &str) -> String {
    let mut diff = format!(
        "--- a/{file}\n+++ b/{file}\n@@ -1,{} +1,{} @@\n",
        before.lines().count(),
        after.lines().count()
    );
    for (prefix, source) in [("-", before), ("+", after)] {
        for line in source.lines() {
            diff.push_str(prefix);
            diff.push_str(line);
            diff.push('\n');
        }
        if !source.is_empty() && !source.ends_with('\n') {
            diff.push_str("\\ No newline at end of file\n");
        }
    }
    diff
}
