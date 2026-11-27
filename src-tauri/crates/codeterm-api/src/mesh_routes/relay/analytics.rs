use codeterm_service_core::mesh::types::RelayResponse;

/// Extract `(provider, role)` for `agent_spawned` analytics when a local daemon
/// successfully relays an agent or fresh-general spawn to a remote peer.
pub(super) fn remote_spawn_analytics_from_relay(
    method: &str,
    path: &str,
    body: &serde_json::Value,
    resp: &RelayResponse,
    default_provider: &str,
) -> Option<(String, Option<String>)> {
    if !resp.ok || !method.eq_ignore_ascii_case("POST") {
        return None;
    }
    let bare = path.split('?').next().unwrap_or(path);
    match bare {
        "/api/agents/spawn" => {
            let data = resp.data.as_ref();
            let provider = data
                .and_then(|d| d.get("agent").and_then(|v| v.as_str()))
                .or_else(|| body.get("agent").and_then(|v| v.as_str()))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(default_provider);
            let role = data
                .and_then(|d| {
                    d.get("role_profile")
                        .and_then(|v| v.as_str())
                        .or_else(|| d.get("role").and_then(|v| v.as_str()))
                })
                .or_else(|| {
                    body.get("role_profile")
                        .and_then(|v| v.as_str())
                        .or_else(|| body.get("role").and_then(|v| v.as_str()))
                })
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            Some((provider.to_string(), role))
        }
        "/api/agents/general/sessions" => {
            let provider = body
                .get("providerId")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(default_provider);
            Some((provider.to_string(), Some("general_agent".to_string())))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_resp(data: serde_json::Value) -> RelayResponse {
        RelayResponse {
            ok: true,
            status: 200,
            data: Some(data),
            error: None,
        }
    }

    #[test]
    fn extracts_provider_and_role_from_relayed_agent_spawn() {
        let resp = ok_resp(serde_json::json!({
            "tab_id": "tab-1",
            "agent": "claude",
            "role": "Worker",
            "role_profile": "worker"
        }));
        assert_eq!(
            remote_spawn_analytics_from_relay(
                "POST",
                "/api/agents/spawn",
                &serde_json::json!({}),
                &resp,
                "codex"
            ),
            Some(("claude".into(), Some("worker".into())))
        );
    }

    #[test]
    fn extracts_general_agent_role_from_relayed_fresh_general_session() {
        let resp = ok_resp(serde_json::json!({ "tab_id": "tab-2" }));
        assert_eq!(
            remote_spawn_analytics_from_relay(
                "POST",
                "/api/agents/general/sessions",
                &serde_json::json!({ "role": "general_agent", "intent": "fresh_general" }),
                &resp,
                "codex"
            ),
            Some(("codex".into(), Some("general_agent".into())))
        );
    }

    #[test]
    fn ignores_failed_or_non_spawn_relays() {
        let failed = RelayResponse {
            ok: false,
            status: 500,
            data: None,
            error: Some("boom".into()),
        };
        assert_eq!(
            remote_spawn_analytics_from_relay(
                "POST",
                "/api/agents/spawn",
                &serde_json::json!({ "agent": "codex" }),
                &failed,
                "codex"
            ),
            None
        );
        assert_eq!(
            remote_spawn_analytics_from_relay(
                "GET",
                "/api/agents/spawn",
                &serde_json::json!({}),
                &ok_resp(serde_json::json!({})),
                "codex"
            ),
            None
        );
    }
}
