use super::*;
use crate::blueprint_document::checker::{self, CheckContext};
use std::collections::BTreeSet;

pub fn check_context(closure: &FrozenClosure, grants: &BTreeSet<String>) -> CheckContext {
    context(closure, &closure.root, grants, 0)
}
fn context(
    closure: &FrozenClosure,
    parsed: &Parsed,
    grants: &BTreeSet<String>,
    depth: usize,
) -> CheckContext {
    let sub = FrozenClosure {
        root: parsed.clone(),
        callees: closure.callees.clone(),
    };
    let (calls, findings) = check_facts(&sub);
    let mut context = CheckContext {
        grants: grants.clone(),
        call_output_media: parsed
            .document
            .nodes()
            .filter_map(|node| {
                let callee = closure.callee(node.call.as_ref()?.pin.as_ref()?).ok()?;
                let port = checker::ports::resolve(
                    callee.document.result.port.as_ref()?,
                    &callee.document.types,
                )
                .ok()?;
                Some((node.id.clone(), port.media?))
            })
            .collect(),
        calls: calls
            .into_iter()
            .map(|(id, call)| {
                (
                    id,
                    checker::VerifiedCall {
                        fixed_arguments: call.fixed_arguments,
                        result_tainted: call.result_tainted,
                        expanded_steps: call.expanded_steps,
                    },
                )
            })
            .collect(),
        call_diagnostics: findings
            .into_iter()
            .map(CallFinding::into_diagnostic)
            .collect(),
        ..Default::default()
    };
    if parsed.document.trigger.is_none()
        && parsed.document.result.port.is_some()
        && !parsed.document.other.is_empty()
    {
        context.call_diagnostics.push(
            CallFinding {
                rule: "C11".into(),
                name: "callable_other".into(),
                field: "other".into(),
                location: parsed.at("other"),
                message: "callable other handlers are not supported; move them to the caller"
                    .into(),
                pin: None,
            }
            .into_diagnostic(),
        );
    }
    if depth >= MAX_CALL_DEPTH {
        return context;
    }
    for (index, node) in parsed.document.nodes().enumerate() {
        let Some(pin) = node.call.as_ref().and_then(|call| call.pin.as_ref()) else {
            continue;
        };
        let Ok(callee) = closure.callee(pin) else {
            continue;
        };
        let allowed = node
            .caps
            .iter()
            .filter(|cap| grants.contains(*cap))
            .cloned()
            .collect();
        let child = callee.parsed();
        let child_context = self::context(closure, &child, &allowed, depth + 1);
        let diagnostics = checker::check_ir(&child.ir(), &child_context);
        if checker::has_errors(&diagnostics) {
            context.calls.remove(&node.id);
            let path = if index < parsed.document.steps.len() {
                format!("steps[{index}].call")
            } else {
                format!("other[{}].call", index - parsed.document.steps.len())
            };
            context.call_diagnostics.push(
                CallFinding {
                    rule: "C10".into(),
                    name: "callee_admission".into(),
                    field: path.clone(),
                    location: parsed.at(&path),
                    message: format!("Callee {}@{} fails admission", pin.id, pin.hash),
                    pin: Some(pin.clone()),
                }
                .into_diagnostic(),
            );
        }
        context
            .call_diagnostics
            .extend(diagnostics.into_iter().filter(|d| d.severity != "info"));
    }
    context
}
