use super::{engine::Session, model::RuntimeFailure};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{RuntimeContext, RuntimeOperation, RuntimeResult};
use serde_json::{json, Value};

pub(super) fn restore_value(config: &Value, thread: &str) -> Value {
    let entries = if config["ownedContext"]["threadId"].as_str() == Some(thread) {
        config["ownedContext"]["entries"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    json!({"instructions":config["systemPrompt"].as_str(),"additionalContext":entries})
}

fn retain(config: &mut Value, thread: &str, value: &Value) {
    let Some(incoming) = value["additionalContext"].as_array() else {
        return;
    };
    if incoming.is_empty() {
        return;
    }
    let mut entries = restore_value(config, thread)["additionalContext"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for entry in incoming {
        if entry["kind"].as_str() != Some("application") {
            log::debug!("[agent-runtime] dropping non-application context from retention");
            continue;
        }
        if let Some(existing) = entries.iter_mut().find(|item| item["key"] == entry["key"]) {
            *existing = entry.clone();
        } else {
            entries.push(entry.clone());
        }
    }
    config["ownedContext"] = json!({"threadId":thread,"entries":entries});
}

pub(super) fn record(
    svc: &ServiceContext,
    session: &Session,
    expected: &RuntimeContext,
    operation: &RuntimeOperation,
    result: &RuntimeResult,
) -> Result<(), RuntimeFailure> {
    let RuntimeOperation::Control { action, value, .. } = operation else {
        return Ok(());
    };
    if action != "restoreContext" || !matches!(result, RuntimeResult::Done) {
        return Ok(());
    }
    let thread = expected
        .thread_id
        .as_deref()
        .ok_or(RuntimeFailure::Superseded)?;
    let mut mux = svc.mux.write();
    {
        let current = session.context.lock();
        if current.epoch != expected.epoch
            || current.binding_nonce != expected.binding_nonce
            || current.thread_id != expected.thread_id
        {
            return Err(RuntimeFailure::Superseded);
        }
    }
    let tab = mux
        .get_tab_mut(&expected.tab_id)
        .ok_or(RuntimeFailure::Superseded)?;
    if tab.session_binding_nonce.as_deref() != Some(expected.binding_nonce.as_str())
        || tab.agent_session_id.as_deref() != Some(thread)
    {
        return Err(RuntimeFailure::Superseded);
    }
    let config = tab.agent_runtime_config.get_or_insert_with(|| json!({}));
    retain(config, thread, value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_acknowledged_context_by_key_and_thread_without_user_turns() {
        let mut config = json!({"systemPrompt":"role-and-assignment"});
        let first = json!({"instructions":null,"additionalContext":[
            {"key":"app","kind":"application","value":"app-value"},
            {"key":"user-context","kind":"user","value":"context-value"}
        ]});
        retain(&mut config, "own", &first);
        retain(&mut config, "own", &first);
        let restored = restore_value(&config, "own");
        assert_eq!(restored["instructions"], "role-and-assignment");
        assert_eq!(
            restored["additionalContext"],
            json!([first["additionalContext"][0]])
        );
        assert!(!restored["additionalContext"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["key"] == "user-context"));
        retain(
            &mut config,
            "own",
            &json!({"additionalContext":[
                {"key":"app","kind":"application","value":"new-value"}
            ]}),
        );
        assert_eq!(
            restore_value(&config, "own")["additionalContext"][0]["value"],
            "new-value"
        );
        assert_eq!(
            restore_value(&config, "foreign")["additionalContext"],
            json!([])
        );
        retain(&mut config, "foreign", &first);
        assert_eq!(
            restore_value(&config, "own")["additionalContext"],
            json!([])
        );
    }
}
