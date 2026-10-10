use serde_json::Value;

pub fn declared_event(status: &Value) -> Option<&'static str> {
    match status["type"].as_str()? {
        "idle" => Some("turn/completed"),
        "active"
            if status["activeFlags"].as_array().is_some_and(|flags| {
                flags.iter().any(|flag| {
                    matches!(
                        flag.as_str(),
                        Some("waitingOnApproval" | "waitingOnUserInput")
                    )
                })
            }) =>
        {
            Some("serverRequest/pending")
        }
        "active" => Some("turn/started"),
        _ => None,
    }
}

pub fn observe(
    svc: &crate::service::ServiceContext,
    tab: &str,
    thread: &str,
    sidecar: &super::lifecycle::Sidecar,
    event: &str,
) {
    match event {
        "turn/completed" => {
            sidecar
                .busy
                .store(false, std::sync::atomic::Ordering::Release);
            svc.mark_agent_turn_complete(tab);
        }
        "turn/started" => {
            sidecar
                .busy
                .store(true, std::sync::atomic::Ordering::Release);
            svc.mark_agent_turn_started(tab);
        }
        "serverRequest/pending" => {
            svc.mark_agent_waiting(tab);
        }
        _ => return,
    }
    crate::agent::state_verdict::observe_declared_event(svc, tab, thread, event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn codex_rpc_control_thread_status_preserves_waiting_idle_and_unknown() {
        assert_eq!(
            declared_event(&json!({"type":"idle"})),
            Some("turn/completed")
        );
        assert_eq!(
            declared_event(&json!({"type":"active","activeFlags":[]})),
            Some("turn/started")
        );
        for flag in ["waitingOnApproval", "waitingOnUserInput"] {
            assert_eq!(
                declared_event(&json!({"type":"active","activeFlags":[flag]})),
                Some("serverRequest/pending")
            );
        }
        assert_eq!(declared_event(&json!({"type":"notLoaded"})), None);
        assert_eq!(declared_event(&json!({"type":"systemError"})), None);
    }
}
