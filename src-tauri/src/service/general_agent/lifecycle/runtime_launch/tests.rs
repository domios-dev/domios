use super::*;

#[test]
fn general_runtime_config_preserves_permanent_context_and_tab_controls() {
    for bypass in [false, true] {
        let config = runtime_config(
            "opaque-general-developer-context".into(),
            Some("opaque-model".into()),
            Some("opaque-effort".into()),
            bypass,
        );
        assert_eq!(config["systemPrompt"], "opaque-general-developer-context");
        assert_eq!(
            crate::service::agent_runtime::model::settings_from_config(&config),
            codeterm_plugin::agent_runtime::RuntimeSettings {
                model: Some("opaque-model".into()),
                effort: Some("opaque-effort".into()),
                mode: Some(
                    if bypass {
                        "full-access"
                    } else {
                        "auto-accept-edits"
                    }
                    .into()
                ),
            }
        );
        assert!(config.get("task").is_none());
        assert!(config.get("env").is_none());
    }
}

#[test]
fn general_runtime_restore_keeps_the_exact_thread_and_fresh_sibling_stays_independent() {
    assert_eq!(
        general_agent_session_plan(true, false, Some("opaque-own-thread".into())),
        GeneralAgentSessionPlan::Restore("opaque-own-thread".into())
    );
    assert_eq!(
        LaunchOwnership::Main.select_session(Some("opaque-own-thread".into())),
        Some("opaque-own-thread".into())
    );
    assert!(LaunchOwnership::FreshSession
        .select_session(Some("opaque-own-thread".into()))
        .is_none());
    assert!(!LaunchOwnership::FreshSession.bootstrap());
}
