use super::*;

fn environment() -> HashMap<String, String> {
    HashMap::from([
        (
            "CODETERM_API_TOKEN".into(),
            "<credential-placeholder>".into(),
        ),
        ("CODETERM_API_URL".into(), "http://127.0.0.1:43210".into()),
        ("CODETERM_TAB_ID".into(), "own".into()),
        ("CODETERM_SESSION_BINDING_NONCE".into(), "nonce".into()),
    ])
}

#[test]
fn agent_runtime_host_mcp_absent_without_minted_local_credential_or_declaration() {
    for (declared, minted) in [(false, false), (false, true), (true, false)] {
        assert!(descriptor(
            declared,
            minted,
            "own",
            "nonce",
            Some(std::path::PathBuf::from("/trusted/domios")),
            &environment(),
        )
        .is_none());
    }
    assert!(descriptor(true, true, "own", "nonce", None, &environment()).is_none());
    let actual = descriptor(
        true,
        true,
        "own",
        "nonce",
        Some(std::path::PathBuf::from("/trusted/domios")),
        &environment(),
    )
    .unwrap();
    assert!(!format!("{actual:?}").contains("<credential-placeholder>"));
    assert_eq!(
        actual.descriptor(),
        serde_json::json!({"name":"domios","command":"/trusted/domios","args":["mcp","serve","--tab","own","--binding-nonce","nonce"],"env":[{"name":"CODETERM_API_TOKEN","value":"<credential-placeholder>"},{"name":"CODETERM_API_URL","value":"http://127.0.0.1:43210"},{"name":"CODETERM_SESSION_BINDING_NONCE","value":"nonce"},{"name":"CODETERM_TAB_ID","value":"own"}]})
    );
}

#[test]
fn agent_runtime_host_mcp_minted_local_refuses_foreign_stale_or_incomplete_environment() {
    use crate::service::tab_credentials::TabCaller;
    use std::collections::HashMap;
    let env = HashMap::from([
        ("CODETERM_API_TOKEN".into(), "opaque-test-credential".into()),
        ("CODETERM_API_URL".into(), "http://127.0.0.1:43210".into()),
        ("CODETERM_TAB_ID".into(), "own".into()),
        ("CODETERM_SESSION_BINDING_NONCE".into(), "nonce".into()),
    ]);
    let own = TabCaller {
        tab_id: "own".into(),
        binding_nonce: "nonce".into(),
    };
    assert!(minted_local(&env, &own, |_| Some(own.clone())));
    for caller in [
        TabCaller {
            tab_id: "foreign".into(),
            ..own.clone()
        },
        TabCaller {
            binding_nonce: "stale".into(),
            ..own.clone()
        },
    ] {
        assert!(!minted_local(&env, &own, |_| Some(caller)));
    }
    for key in [
        "CODETERM_API_TOKEN",
        "CODETERM_API_URL",
        "CODETERM_TAB_ID",
        "CODETERM_SESSION_BINDING_NONCE",
    ] {
        let mut missing = env.clone();
        missing.remove(key);
        assert!(!minted_local(&missing, &own, |_| Some(own.clone())));
        let mut empty = env.clone();
        empty.insert(key.into(), String::new());
        assert!(!minted_local(&empty, &own, |_| Some(own.clone())));
    }
    for key in ["CODETERM_TAB_ID", "CODETERM_SESSION_BINDING_NONCE"] {
        let mut mismatch = env.clone();
        mismatch.insert(key.into(), "foreign".into());
        assert!(!minted_local(&mismatch, &own, |_| Some(own.clone())));
    }
    assert!(!minted_local(&env, &own, |_| None));
}

#[test]
fn agent_runtime_host_mcp_forwards_only_the_tab_task_identity() {
    let mut env = environment();
    env.insert("CODETERM_PLAN_ID".into(), "opaque-plan".into());
    env.insert("CODETERM_TASK_ID".into(), "opaque-task".into());
    env.insert("CODETERM_LAUNCH_FILE".into(), "opaque-file".into());
    env.insert("CODETERM_ROLE".into(), "opaque-role".into());
    let bound = descriptor(
        true,
        true,
        "own",
        "nonce",
        Some("/trusted/domios".into()),
        &env,
    )
    .unwrap();
    assert_eq!(
        bound.env.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "CODETERM_API_TOKEN",
            "CODETERM_API_URL",
            "CODETERM_PLAN_ID",
            "CODETERM_SESSION_BINDING_NONCE",
            "CODETERM_TAB_ID",
            "CODETERM_TASK_ID",
        ]
    );
    assert_eq!(bound.env["CODETERM_TASK_ID"], "opaque-task");
    env.insert("CODETERM_TASK_ID".into(), " ".into());
    let unbound = descriptor(
        true,
        true,
        "own",
        "nonce",
        Some("/trusted/domios".into()),
        &env,
    )
    .unwrap();
    assert!(!unbound.env.contains_key("CODETERM_TASK_ID"));
}
