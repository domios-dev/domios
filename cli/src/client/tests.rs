use super::*;

#[test]
fn connect_failure_diagnostic_names_the_caller_tab_and_distinguishes_denial() {
    let path = "codeterm-fixture.sock";
    let plain = connect_failure_message(path, false, None);
    let agent = connect_failure_message(path, false, Some("fixture-tab"));
    let denied_agent = connect_failure_message(path, true, Some("fixture-tab"));
    let denied = connect_failure_message(path, true, None);

    assert!(plain.contains(path));
    assert!(!plain.contains("fixture-tab"));
    assert!(agent.contains("fixture-tab"));
    assert!(denied_agent.contains("fixture-tab"));
    assert!(!denied.contains("fixture-tab"));
    for message in [&agent, &denied_agent, &denied] {
        assert!(message.starts_with(&plain));
    }
    assert_ne!(agent, denied_agent);
    assert_ne!(plain, denied);
}

// The helper under test only exists off Windows, where /tmp socket files do.
#[cfg(not(target_os = "windows"))]
#[test]
fn build_gateway_mutex_files_are_not_daemon_sockets() {
    assert!(!is_daemon_socket_name(
        "codeterm-gateway-mutex-13026-2189552-cargo.sock"
    ));
    assert!(!is_daemon_socket_name(
        "codeterm-gateway-mutex-13026-2189552-rustc.sock"
    ));
    assert!(is_daemon_socket_name("codeterm-49765.sock"));
    assert!(is_daemon_socket_name("codeterm-dev.sock"));
    assert!(!is_daemon_socket_name("codeterm-49765"));
    assert!(!is_daemon_socket_name("other-49765.sock"));
}

#[cfg(not(target_os = "windows"))]
#[test]
fn stale_socket_file_falls_through_to_live_probe() {
    use std::cell::RefCell;

    let entry_names = vec![
        "codeterm-gateway-mutex-13026-2189552-cargo.sock".to_string(),
        "codeterm-777.sock".to_string(),
        "unrelated.sock".to_string(),
    ];
    let liveness_checked: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let probe_calls = RefCell::new(0usize);

    let resolved = resolve_scanned_socket(
        entry_names.into_iter(),
        &|name| {
            liveness_checked.borrow_mut().push(name.to_string());
            false
        },
        &|candidates| pick_preferred_socket_for(candidates, false, None),
        &|| {
            *probe_calls.borrow_mut() += 1;
            Some("codeterm-4242.sock".to_string())
        },
    );

    assert_eq!(
        *probe_calls.borrow(),
        1,
        "a stale socket file must not stop discovery before the live probe"
    );
    assert_eq!(resolved, Ok("codeterm-4242.sock".to_string()));
    assert_eq!(
        *liveness_checked.borrow(),
        vec!["codeterm-777.sock".to_string()],
        "only daemon-named candidates may reach the liveness check"
    );
}

#[cfg(not(target_os = "windows"))]
#[test]
fn live_socket_file_wins_without_probing() {
    use std::cell::RefCell;

    let probe_calls = RefCell::new(0usize);
    let resolved = resolve_scanned_socket(
        vec!["codeterm-777.sock".to_string()].into_iter(),
        &|_| true,
        &|candidates| pick_preferred_socket_for(candidates, false, None),
        &|| {
            *probe_calls.borrow_mut() += 1;
            Some("codeterm-4242.sock".to_string())
        },
    );

    assert_eq!(resolved, Ok("codeterm-777.sock".to_string()));
    assert_eq!(*probe_calls.borrow(), 0);
}

#[test]
fn picks_dev_when_dev_preferred() {
    let candidates = vec!["codeterm-1234".to_string(), "codeterm-dev.sock".to_string()];
    assert_eq!(
        pick_preferred_socket(&candidates, true),
        Some("codeterm-dev.sock".to_string())
    );
}

#[test]
fn picks_prod_when_dev_not_preferred() {
    let candidates = vec!["codeterm-1234".to_string(), "codeterm-dev.sock".to_string()];
    assert_eq!(
        pick_preferred_socket(&candidates, false),
        Some("codeterm-1234".to_string())
    );
}

#[test]
fn falls_back_when_only_other_kind_present() {
    let candidates = vec!["codeterm-1234".to_string()];
    assert_eq!(
        pick_preferred_socket(&candidates, true),
        Some("codeterm-1234".to_string())
    );
}

#[test]
fn returns_none_when_nothing_found() {
    let candidates: Vec<String> = vec![];
    assert_eq!(pick_preferred_socket(&candidates, true), None);
    assert_eq!(pick_preferred_socket(&candidates, false), None);
}

#[test]
fn instance_socket_wins_over_plain_dev() {
    let candidates = vec![
        "codeterm-dev.sock".to_string(),
        "codeterm-dev-exp.sock".to_string(),
    ];
    assert_eq!(
        pick_preferred_socket_for(&candidates, true, Some("codeterm-dev-exp")),
        Some("codeterm-dev-exp.sock".to_string())
    );
}

#[test]
fn instance_falls_back_to_dev_when_own_absent() {
    let candidates = vec!["codeterm-dev.sock".to_string()];
    assert_eq!(
        pick_preferred_socket_for(&candidates, true, Some("codeterm-dev-exp")),
        Some("codeterm-dev.sock".to_string())
    );
}

#[test]
fn deterministic_socket_uses_named_instance() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let prev_instance = std::env::var("CODETERM_INSTANCE").ok();
    let prev_port = std::env::var("CODETERM_API_PORT").ok();
    std::env::set_var("CODETERM_INSTANCE", "handoff");
    std::env::set_var("CODETERM_API_PORT", "7691");

    assert_eq!(
        deterministic_socket_name().as_deref(),
        Some("codeterm-dev-handoff.sock")
    );
    assert_eq!(
        deterministic_socket_resolution().map(|resolved| resolved.source),
        Some(ResolutionSource::CodeTermInstance)
    );

    restore_env("CODETERM_INSTANCE", prev_instance);
    restore_env("CODETERM_API_PORT", prev_port);
}

#[test]
fn deterministic_socket_uses_plain_dev_port() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let prev_instance = std::env::var("CODETERM_INSTANCE").ok();
    let prev_port = std::env::var("CODETERM_API_PORT").ok();
    std::env::remove_var("CODETERM_INSTANCE");
    std::env::set_var("CODETERM_API_PORT", "7686");

    assert_eq!(
        deterministic_socket_name().as_deref(),
        Some("codeterm-dev.sock")
    );
    assert_eq!(
        deterministic_socket_resolution().map(|resolved| resolved.source),
        Some(ResolutionSource::CodeTermApiPort)
    );

    restore_env("CODETERM_INSTANCE", prev_instance);
    restore_env("CODETERM_API_PORT", prev_port);
}

#[test]
fn deterministic_socket_treats_non_prod_port_as_dev() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let prev_instance = std::env::var("CODETERM_INSTANCE").ok();
    let prev_port = std::env::var("CODETERM_API_PORT").ok();
    std::env::remove_var("CODETERM_INSTANCE");
    std::env::set_var("CODETERM_API_PORT", "7694");

    assert_eq!(
        deterministic_socket_name().as_deref(),
        Some("codeterm-dev.sock")
    );
    assert!(!socket_path_matches_port_pref("codeterm-12345.sock", true));

    restore_env("CODETERM_INSTANCE", prev_instance);
    restore_env("CODETERM_API_PORT", prev_port);
}

#[test]
fn socket_pref_match_dev_pipe_with_dev_pref() {
    assert!(socket_path_matches_port_pref("codeterm-dev.sock", true));
    assert!(socket_path_matches_port_pref("codeterm-dev", true));
}

#[test]
fn socket_pref_match_prod_pid_with_prod_pref() {
    assert!(socket_path_matches_port_pref("codeterm-12345.sock", false));
    assert!(socket_path_matches_port_pref("codeterm-9876", false));
}

#[test]
fn socket_pref_mismatch_when_prod_pipe_but_dev_requested() {
    assert!(!socket_path_matches_port_pref("codeterm-12345.sock", true));
}

#[test]
fn socket_pref_mismatch_when_dev_pipe_but_prod_requested() {
    assert!(!socket_path_matches_port_pref("codeterm-dev.sock", false));
}

#[test]
fn is_dev_socket_does_not_match_pid_paths_with_dev_substring() {
    // `contains("dev")` would have classified `codeterm-99987devops.sock` as dev — guard against that.
    assert!(!is_dev_socket("codeterm-99987devops.sock"));
    assert!(!is_dev_socket("codeterm-developerlaptop.sock"));
    assert!(is_dev_socket("codeterm-dev"));
    assert!(is_dev_socket("codeterm-dev.sock"));
}

fn method_not_found() -> SocketCallError {
    SocketCallError::Rpc(RpcError {
        code: -32601,
        message: "Method not found".into(),
        data: None,
    })
}

#[test]
fn version_skew_same_version_call_makes_exactly_one_request() {
    let mut seen: Vec<(String, Option<serde_json::Value>)> = Vec::new();
    let params = serde_json::json!({"tab_id": "t1"});

    let result = call_with_version_fallback(
        |method, params| {
            seen.push((method.to_string(), params));
            Ok(serde_json::json!({"ok": true}))
        },
        "tab.list",
        Some(params.clone()),
    )
    .unwrap();

    assert_eq!(result["ok"], true);
    assert_eq!(seen, vec![("tab.list".to_string(), Some(params))]);
}

#[test]
fn version_skew_typed_error_passes_through_untranslated() {
    let mut calls = 0;
    let outcome = call_with_version_fallback(
        |_, _| {
            calls += 1;
            Err(SocketCallError::Rpc(RpcError {
                code: crate::protocol::VERSION_SKEW_CODE,
                message: "client protocol v1, server requires >=2 — run rebuild".into(),
                data: Some(serde_json::json!({
                    "type": "version_skew",
                    "client_protocol": 1,
                    "server_protocol": 2,
                    "recovery": "rebuild",
                })),
            }))
        },
        "agent.report",
        None,
    );

    let SocketCallError::Rpc(error) = outcome.unwrap_err() else {
        panic!("the typed error must stay an RPC error");
    };
    assert_eq!(calls, 1, "a skew refusal must not trigger extra requests");
    assert_eq!(error.code, crate::protocol::VERSION_SKEW_CODE);
    let data = error.data.expect("typed data crosses untranslated");
    assert_eq!(data["client_protocol"], 1);
    assert_eq!(data["server_protocol"], 2);
    assert_eq!(data["recovery"], "rebuild");
}

#[test]
fn version_skew_error_renders_the_daemon_message_verbatim() {
    let message = "client protocol v3, server requires >=5 — run ./fix";

    let rendered = SocketCallError::Rpc(RpcError {
        code: crate::protocol::VERSION_SKEW_CODE,
        message: message.into(),
        data: None,
    })
    .to_string();
    assert_eq!(rendered, message);

    let other = SocketCallError::Rpc(RpcError {
        code: -32601,
        message: message.into(),
        data: None,
    })
    .to_string();
    assert_ne!(other, message, "only version skew renders bare");
}

#[test]
fn version_skew_request_envelope_carries_the_protocol_integer() {
    let req = JsonRpcRequest {
        jsonrpc: "2.0",
        id: 1,
        method: "tab.list",
        params: None,
        protocol_version: crate::protocol::PROTOCOL_VERSION,
    };
    let wire: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
    assert_eq!(wire["protocol_version"], crate::protocol::PROTOCOL_VERSION);
}

#[test]
fn version_skew_unknown_method_fallback_names_both_versions() {
    let mut seen: Vec<String> = Vec::new();
    let outcome = call_with_version_fallback(
        |method, _| {
            seen.push(method.to_string());
            if method == "system.identify" {
                Ok(serde_json::json!({"version": "0.0.1"}))
            } else {
                Err(method_not_found())
            }
        },
        "tab.list",
        None,
    );

    assert_eq!(
        seen,
        vec!["tab.list".to_string(), "system.identify".to_string()]
    );
    let SocketCallError::Transport(rendered) = outcome.unwrap_err() else {
        panic!("cross-version -32601 becomes a transport explanation");
    };
    assert!(rendered.contains("0.0.1"), "{rendered}");
    assert!(rendered.contains(env!("CARGO_PKG_VERSION")), "{rendered}");
    assert!(rendered.contains("tab.list"), "{rendered}");
}

#[test]
fn version_skew_unknown_method_on_same_version_daemon_stays_method_not_found() {
    let outcome = call_with_version_fallback(
        |method, _| {
            if method == "system.identify" {
                Ok(serde_json::json!({"version": env!("CARGO_PKG_VERSION")}))
            } else {
                Err(method_not_found())
            }
        },
        "tab.list",
        None,
    );

    let SocketCallError::Rpc(error) = outcome.unwrap_err() else {
        panic!("same-version -32601 is a real missing method, not skew");
    };
    assert_eq!(error.code, -32601);
}

#[test]
fn socket_transport_remains_primary_when_available() {
    let choice = select_transport::<()>(Ok(()), || {
        panic!("fallback config must not be read when socket connected")
    });
    assert!(matches!(choice, Ok(TransportChoice::Socket(()))));
}

#[test]
fn dead_socket_uses_loopback_http_when_credentials_are_present() {
    let config = http_fallback_config_from(None, Some("7687"), Some("opaque-token"))
        .expect("port and API token configure HTTP fallback");
    let choice = select_transport::<()>(Err("pipe denied".into()), || Ok(config));
    let TransportChoice::Http(config) = choice.unwrap() else {
        panic!("HTTP fallback expected");
    };
    assert_eq!(config.base_url, "http://127.0.0.1:7687");
}

#[test]
fn current_pad_dead_socket_with_http_env_uses_shared_transport_selection() {
    struct PadClient(std::path::PathBuf);

    impl RpcClient for PadClient {
        fn call_detailed(
            &mut self,
            method: &str,
            _params: Option<serde_json::Value>,
        ) -> Result<serde_json::Value, SocketCallError> {
            assert_eq!(method, "tab.agent_pad");
            Ok(serde_json::json!({ "path": self.0 }))
        }
    }

    let config = http_fallback_config_from(None, Some("7687"), Some("opaque-token")).unwrap();
    let choice = select_transport::<()>(Err("pipe denied".into()), || Ok(config)).unwrap();
    assert!(matches!(choice, TransportChoice::Http(_)));

    let temp = tempfile::tempdir().unwrap();
    let tab_id = "full-worker-tab-id";
    let pad = temp.path().join(tab_id);
    std::fs::create_dir(&pad).unwrap();
    let mut client = PadClient(pad.clone());
    assert_eq!(
        crate::cmd::payload_source::resolve_current_pad(tab_id, &mut client).unwrap(),
        pad.canonicalize().unwrap()
    );
}

#[test]
fn dead_socket_without_credentials_reports_both_transport_attempts() {
    let error = match select_transport::<()>(Err("pipe denied".into()), || {
        Err("CODETERM_API_TOKEN is not set".into())
    }) {
        Ok(_) => panic!("missing HTTP credentials must fail selection"),
        Err(error) => error,
    };
    assert!(
        error.contains("socket attempt failed: pipe denied"),
        "{error}"
    );
    assert!(error.contains("loopback HTTP attempt failed"), "{error}");
}

#[test]
fn http_fallback_refuses_non_loopback_urls() {
    let error = match http_fallback_config_from(
        Some("http://192.168.1.2:7687"),
        Some("7687"),
        Some("opaque-token"),
    ) {
        Ok(_) => panic!("non-loopback URLs must be rejected"),
        Err(error) => error,
    };
    assert!(error.contains("127.0.0.1"));
    assert!(!error.contains("opaque-token"));
}

#[test]
fn http_rpc_result_preserves_payloads_with_an_inner_error_field() {
    let result = http_rpc_result(
        reqwest::StatusCode::OK,
        serde_json::json!({
            "result": { "error": { "kind": "report_refused" }, "accepted": false }
        }),
    )
    .unwrap();
    assert_eq!(result["error"]["kind"], "report_refused");
    assert_eq!(result["accepted"], false);
}

fn http_config(base_url: &str) -> HttpFallbackConfig {
    HttpFallbackConfig {
        base_url: base_url.to_owned(),
        token: "opaque-token".into(),
    }
}

fn restore_env(key: &str, value: Option<String>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}
