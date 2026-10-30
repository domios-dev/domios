use super::*;
use serde_json::json;

fn catalog() -> AgentVerbCatalog {
    AgentVerbCatalog::compose(
        "opaque-messenger",
        "Fixture Messenger",
        Some("Read messages"),
        &[
            "login".into(),
            "login-status".into(),
            "health".into(),
            "chats [query]".into(),
        ],
        None,
        &[],
    )
}

#[test]
fn login_refusal_has_exact_command_and_chat_first_code_guidance() {
    for error in [
        "not-logged-in: connect first",
        "not-installed: helper missing",
        "reauth-needed: expired",
    ] {
        let refusal = actionable_refusal(&catalog(), error);
        assert!(refusal.starts_with(error));
        assert!(refusal.contains("`domios plugin opaque-messenger login`"));
        assert!(refusal.contains("chat-first sign-in"));
        assert!(refusal.contains("sign-in code or QR"));
        assert!(refusal.contains("to the user in chat"));
    }
    assert_eq!(
        actionable_refusal(&catalog(), "upstream-rejected: offline"),
        "upstream-rejected: offline"
    );
    let mut no_login = catalog();
    no_login.verbs.clear();
    assert_eq!(
        actionable_refusal(&no_login, "not-installed: missing"),
        "not-installed: missing"
    );
}

#[test]
fn connection_snapshot_retains_only_known_states() {
    for (value, expected) in [
        (
            json!({"state":"logged-in", "account":"private", "deviceCode":"private"}),
            Some(PluginConnectionState::SignedIn),
        ),
        (
            json!({"result":"{\"state\":\"logged-out\"}"}),
            Some(PluginConnectionState::SignedOut),
        ),
        (
            json!({"state":"not-installed"}),
            Some(PluginConnectionState::RuntimeMissing),
        ),
        (
            json!({"state":"awaiting-user"}),
            Some(PluginConnectionState::Pending),
        ),
        (json!({"state":"sent"}), None),
        (json!({"error":"unknown: logged-in"}), None),
    ] {
        assert_eq!(PluginConnectionState::from_response(&value), expected);
    }
}

#[test]
fn recall_action_fits_existing_default_snippet_budget() {
    let mut catalog = catalog();
    for (state, command) in [
        (PluginConnectionState::SignedIn, "--help"),
        (PluginConnectionState::SignedOut, "login"),
        (PluginConnectionState::RuntimeMissing, "login"),
        (PluginConnectionState::Unknown, "login-status"),
    ] {
        catalog.connection_state = Some(state);
        let line = recall_line(&catalog);
        assert!(line.contains(state.label()));
        assert!(line.contains(&format!("`domios plugin opaque-messenger {command}`")));
        assert!(line.chars().count() <= 140);
    }
}

#[test]
fn unknown_and_pending_recall_only_suggest_declared_status_verbs_or_help() {
    for (verbs, expected) in [
        (
            vec!["login", "login-status", "health", "status"],
            "login-status",
        ),
        (vec!["login", "health", "logout"], "health"),
        (vec!["login", "status"], "status"),
        (vec!["login"], "--help"),
    ] {
        let mut catalog = catalog();
        catalog.verbs = verbs.iter().map(|verb| (*verb).to_owned()).collect();
        for state in [
            None,
            Some(PluginConnectionState::Unknown),
            Some(PluginConnectionState::Pending),
        ] {
            catalog.connection_state = state;
            let line = recall_line(&catalog);
            let command = line.split('`').nth(1).expect("suggested command");
            let verb = command
                .split_whitespace()
                .last()
                .expect("verb or help option");
            assert_eq!(verb, expected);
            assert!(verb == "--help" || catalog.verbs.iter().any(|declared| declared == verb));
            assert!(line.contains(state.unwrap_or(PluginConnectionState::Unknown).label()));
        }
    }
}

#[test]
fn auth_refusal_takes_precedence_over_signed_in_state() {
    for response in [
        json!({"state":"logged-in", "error":"not-logged-in: connect first"}),
        json!({"state":"logged-in", "error":"reauth-needed: expired"}),
        json!({"result":"{\"state\":\"logged-in\"}", "error":"not-logged-in: connect first"}),
    ] {
        assert_eq!(
            PluginConnectionState::from_response(&response),
            Some(PluginConnectionState::SignedOut)
        );
        let mut catalog = catalog();
        catalog.connection_state = PluginConnectionState::from_response(&response);
        assert!(recall_line(&catalog)
            .contains("not signed in. Offer sign-in: `domios plugin opaque-messenger login`"));
    }
    assert_eq!(
        PluginConnectionState::from_response(
            &json!({"state":"logged-in", "error":"upstream-rejected: offline"})
        ),
        Some(PluginConnectionState::SignedIn)
    );
}
