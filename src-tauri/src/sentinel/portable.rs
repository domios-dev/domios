use std::collections::BTreeMap;

use codeterm_types::blueprint_recipe::{Recipe, SCHEMA_VERSION};
use serde_json::{json, Value};

use super::{
    handlers::Handler,
    machine::{EscalateTarget, Route},
    store::Sentinel,
};

pub fn export(job: &Sentinel) -> Result<Recipe, String> {
    let mut placeholders = BTreeMap::new();
    let mut definition = definition(job);
    if job.pipeline().is_none() {
        definition["create"]["timeout"] = json!("{{timeout}}");
        placeholders.insert(
            "timeout".into(),
            "Authored lifetime duration, for example 1h".into(),
        );
    }
    redact(&mut definition, "definition", &mut placeholders);
    let recipe = Recipe {
        schema_version: SCHEMA_VERSION,
        placeholders,
        definition,
    };
    recipe.validate()?;
    Ok(recipe)
}

fn definition(job: &Sentinel) -> Value {
    if let Some(pipeline) = &job.pipeline().cloned() {
        json!({"spec": pipeline})
    } else {
        let mut handlers = BTreeMap::new();
        for (bucket, routes) in &job.routes {
            handlers.insert(
                bucket.clone(),
                routes.iter().map(handler).collect::<Vec<_>>(),
            );
        }
        if !job.on_other.is_empty() {
            handlers.insert("other".into(), job.on_other.iter().map(handler).collect());
        }
        json!({"create": {
            "name": job.name,
            "terminal": job.terminal().cloned(),
            "session": job.session,
            "probe": if job.probe().is_empty() { None } else { Some(job.probe()) },
            "observe": job.observe().cloned(),
            "input": job.input().cloned(),
            "question": job.question,
            "buckets": job.buckets,
            "every": format!("{}s", job.interval_ms / 1000),
            "handlers": handlers,
            "matches": job.matches,
            "rearm_on_change": job.rearm_on_change,
            "until": job.until,
            "max_transitions": job.max_transitions,
            "max_actions": job.max_actions,
            "escalation_timeout_ms": job.escalation_timeout_ms
        }})
    }
}

fn handler(handler: &Handler) -> Value {
    let (kind, parameters) = match &handler.action {
        Route::Notify => ("notify", json!({})),
        Route::Stop => ("stop", json!({})),
        Route::Send(target) => ("send", json!({"target": target})),
        Route::Cron(target) => ("cron", json!({"target": target})),
        Route::Sentinel(target) => ("blueprint", json!({"target": target})),
        Route::RespondPrompt(kind) => ("respond_prompt", json!({"target": kind.as_str()})),
        Route::Escalate(EscalateTarget::Creator) => ("escalate", json!({"target": "creator"})),
        Route::Escalate(EscalateTarget::Agent(target)) => {
            ("escalate", json!({"target": format!("agent:{target}")}))
        }
        Route::Command(parameters) => (
            "command",
            json!({"command": parameters.command, "label": parameters.label, "timeout_ms": parameters.timeout_ms}),
        ),
    };
    json!({"id": handler.id, "type": kind, "parameters": parameters})
}

fn redact(value: &mut Value, path: &str, placeholders: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                let next = format!("{path}.{key}");
                if matches!(
                    key.as_str(),
                    "probe"
                        | "command"
                        | "params"
                        | "source"
                        | "data"
                        | "question"
                        | "matches"
                        | "pattern"
                        | "target"
                        | "id"
                        | "label"
                        | "to"
                ) && !value.is_null()
                {
                    redact_payload(value, &next, placeholders);
                } else {
                    redact(value, &next, placeholders);
                }
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter_mut().enumerate() {
                redact(value, &format!("{path}.{index}"), placeholders);
            }
        }
        _ => {}
    }
}

fn redact_payload(value: &mut Value, path: &str, placeholders: &mut BTreeMap<String, String>) {
    match value {
        Value::String(text) if !text.is_empty() => {
            let key = format!("binding_{}", placeholders.len() + 1);
            placeholders.insert(key.clone(), format!("Value for {path}"));
            *text = format!("{{{{{key}}}}}");
        }
        Value::Array(values) => {
            for (index, value) in values.iter_mut().enumerate() {
                redact_payload(value, &format!("{path}.{index}"), placeholders);
            }
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                redact_payload(value, &format!("{path}.{key}"), placeholders);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
