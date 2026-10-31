use super::*;

#[test]
fn connect_failure_diagnostic_names_the_caller_tab_and_distinguishes_denial() {
    let path = "domios-fixture.sock";
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

#[test]
fn server_internal_channels_are_not_daemon_sockets() {
    let hash = "a".repeat(64);
    assert!(!is_daemon_socket_name(&format!(
        "domios-bootstrap-{hash}.sock"
    )));
    assert!(!is_daemon_socket_name(&format!(
        "domios-native-host-{hash}.sock"
    )));
    assert!(is_daemon_socket_name("domios-49765.sock"));
    assert!(is_daemon_socket_name("domios-dev.sock"));
    assert!(is_daemon_socket_name("domios-dev-exp.sock"));
    assert!(!is_daemon_socket_name("domios-.sock"));
    assert!(!is_daemon_socket_name("domios-dev-.sock"));

    let scanned: Vec<String> = [
        format!("domios-bootstrap-{hash}.sock"),
        format!("domios-native-host-{hash}.sock"),
        "codeterm-4242.sock".to_string(),
    ]
    .into_iter()
    .filter(|name| is_daemon_socket_name(name))
    .collect();
    assert_eq!(pick_preferred_socket_for(&scanned, false, None), None);
}

#[test]
fn build_gateway_mutex_files_are_not_daemon_sockets() {
    assert!(!is_daemon_socket_name(
        "codeterm-gateway-mutex-13026-2189552-cargo.sock"
    ));
    assert!(!is_daemon_socket_name(
        "codeterm-gateway-mutex-13026-2189552-rustc.sock"
    ));
    assert!(is_daemon_socket_name("domios-49765.sock"));
    assert!(is_daemon_socket_name("domios-dev.sock"));
    assert!(!is_daemon_socket_name("domios-49765"));
    assert!(!is_daemon_socket_name("other-49765.sock"));
}

#[cfg(not(target_os = "windows"))]
#[test]
fn stale_socket_file_falls_through_to_live_probe() {
    use std::cell::RefCell;

    let entry_names = vec![
        "codeterm-gateway-mutex-13026-2189552-cargo.sock".to_string(),
        "domios-777.sock".to_string(),
        "unrelated.sock".to_string(),
    ];
    let liveness_checked: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let probe_calls = RefCell::new(0usize);

    let resolved = resolve_scanned_socket(
        entry_names.into_iter(),
        &|name| {
            liveness_checked.borrow_mut().push(name.to_string());
            None::<()>
        },
        &|candidates| pick_preferred_socket_for(candidates, false, None),
        &|| {
            *probe_calls.borrow_mut() += 1;
            Some(((), "domios-4242.sock".to_string()))
        },
    );

    assert_eq!(
        *probe_calls.borrow(),
        1,
        "a stale socket file must not stop discovery before the live probe"
    );
    assert_eq!(resolved, Ok(((), "domios-4242.sock".to_string())));
    assert_eq!(
        *liveness_checked.borrow(),
        vec!["domios-777.sock".to_string()],
        "only daemon-named candidates may reach the liveness check"
    );
}

#[cfg(not(target_os = "windows"))]
#[test]
fn live_socket_file_wins_without_probing() {
    use std::cell::RefCell;

    let probe_calls = RefCell::new(0usize);
    let resolved = resolve_scanned_socket(
        vec!["domios-777.sock".to_string()].into_iter(),
        &|_| Some(()),
        &|candidates| pick_preferred_socket_for(candidates, false, None),
        &|| {
            *probe_calls.borrow_mut() += 1;
            Some(((), "domios-4242.sock".to_string()))
        },
    );

    assert_eq!(resolved, Ok(((), "domios-777.sock".to_string())));
    assert_eq!(*probe_calls.borrow(), 0);
}

#[test]
fn picks_dev_when_dev_preferred() {
    let candidates = vec!["domios-1234".to_string(), "domios-dev.sock".to_string()];
    assert_eq!(
        pick_preferred_socket(&candidates, true),
        Some("domios-dev.sock".to_string())
    );
}

#[test]
fn picks_prod_when_dev_not_preferred() {
    let candidates = vec!["domios-1234".to_string(), "domios-dev.sock".to_string()];
    assert_eq!(
        pick_preferred_socket(&candidates, false),
        Some("domios-1234".to_string())
    );
}

#[test]
fn falls_back_when_only_other_kind_present() {
    let candidates = vec!["domios-1234".to_string()];
    assert_eq!(
        pick_preferred_socket(&candidates, true),
        Some("domios-1234".to_string())
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
        "domios-dev.sock".to_string(),
        "domios-dev-exp.sock".to_string(),
    ];
    assert_eq!(
        pick_preferred_socket_for(&candidates, true, Some("domios-dev-exp")),
        Some("domios-dev-exp.sock".to_string())
    );
}

#[test]
fn instance_falls_back_to_dev_when_own_absent() {
    let candidates = vec!["domios-dev.sock".to_string()];
    assert_eq!(
        pick_preferred_socket_for(&candidates, true, Some("domios-dev-exp")),
        Some("domios-dev.sock".to_string())
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
        Some("domios-dev-handoff.sock")
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
        Some("domios-dev.sock")
    );
    assert_eq!(
        deterministic_socket_resolution().map(|resolved| resolved.source),
        Some(ResolutionSource::CodeTermApiPort)
    );

    restore_env("CODETERM_INSTANCE", prev_instance);
    restore_env("CODETERM_API_PORT", prev_port);
}

#[test]
fn custom_port_does_not_guess_the_default_dev_socket() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let prev_instance = std::env::var("CODETERM_INSTANCE").ok();
    let prev_port = std::env::var("CODETERM_API_PORT").ok();
    std::env::remove_var("CODETERM_INSTANCE");
    std::env::set_var("CODETERM_API_PORT", "7694");

    assert_eq!(deterministic_socket_name(), None);
    assert!(!socket_path_matches_port_pref("domios-12345.sock", true));

    restore_env("CODETERM_INSTANCE", prev_instance);
    restore_env("CODETERM_API_PORT", prev_port);
}

#[test]
fn socket_resolution_handles_each_environment_combination() {
    let cases = [
        (None, None, None, None),
        (
            Some("domios-dev-coreqa.sock"),
            None,
            None,
            Some("domios-dev-coreqa.sock"),
        ),
        (None, None, Some("7686"), Some("domios-dev.sock")),
        (None, Some("coreqa"), None, Some("domios-dev-coreqa.sock")),
        (
            Some("domios-dev-coreqa.sock"),
            None,
            Some("7712"),
            Some("domios-dev-coreqa.sock"),
        ),
        (
            Some("domios-dev-coreqa.sock"),
            Some("coreqa"),
            None,
            Some("domios-dev-coreqa.sock"),
        ),
        (
            None,
            Some("coreqa"),
            Some("7712"),
            Some("domios-dev-coreqa.sock"),
        ),
        (
            Some("domios-dev-coreqa.sock"),
            Some("coreqa"),
            Some("7712"),
            Some("domios-dev-coreqa.sock"),
        ),
    ];

    for (socket, instance, port, expected) in cases {
        let resolved = resolve_socket_selection(socket, instance, port).unwrap();
        assert_eq!(resolved.map(|value| value.path).as_deref(), expected);
        if socket.is_some() {
            assert_eq!(
                resolve_socket_selection(socket, instance, port)
                    .unwrap()
                    .map(|value| value.source),
                Some(ResolutionSource::CodeTermSocketPath)
            );
        }
    }
}

#[test]
fn explicit_socket_is_not_replaced_by_a_derived_socket() {
    let resolved = resolve_socket_selection(Some("domios-dev-coreqa.sock"), None, Some("7712"))
        .unwrap()
        .unwrap();

    assert_eq!(resolved.path, "domios-dev-coreqa.sock");
    assert_eq!(resolved.source, ResolutionSource::CodeTermSocketPath);
}

#[test]
fn explicit_named_socket_supplies_the_instance_when_env_is_absent() {
    assert_eq!(
        instance_socket_name_from_path("/tmp/domios-dev-coreqa.sock").as_deref(),
        Some("coreqa")
    );
    assert_eq!(
        instance_socket_name_from_path("\\\\.\\pipe\\domios-dev-coreqa.sock").as_deref(),
        Some("coreqa")
    );
    assert_eq!(instance_socket_name_from_path("domios-dev.sock"), None);
    assert_eq!(
        instance_socket_name_from_path("domios-dev-CoreQA.sock"),
        None
    );
}

#[test]
fn conflicting_socket_instance_and_port_selectors_are_refused() {
    let mismatch_cases = [
        (Some("domios-dev-other.sock"), Some("coreqa"), Some("7712")),
        (Some("domios-dev-other.sock"), Some("coreqa"), None),
        (Some("domios-12345.sock"), None, Some("7712")),
        (Some("custom.sock"), None, Some("7712")),
        (Some("domios-dev-coreqa.sock"), None, Some("7685")),
        (Some("domios-dev-coreqa.sock"), None, Some("7686")),
        (Some("domios-dev.sock"), None, Some("7712")),
        (None, Some("coreqa"), Some("7685")),
        (None, Some("coreqa"), Some("7686")),
        (None, None, Some("7712")),
    ];

    for (socket, instance, port) in mismatch_cases {
        assert!(
            resolve_socket_selection(socket, instance, port).is_err(),
            "accepted conflicting socket={socket:?}, instance={instance:?}, port={port:?}"
        );
    }
}

#[test]
fn socket_pref_match_dev_pipe_with_dev_pref() {
    assert!(socket_path_matches_port_pref("domios-dev.sock", true));
    assert!(socket_path_matches_port_pref("domios-dev", true));
}

#[test]
fn socket_pref_match_prod_pid_with_prod_pref() {
    assert!(socket_path_matches_port_pref("domios-12345.sock", false));
    assert!(socket_path_matches_port_pref("domios-9876", false));
}

#[test]
fn socket_pref_mismatch_when_prod_pipe_but_dev_requested() {
    assert!(!socket_path_matches_port_pref("domios-12345.sock", true));
}

#[test]
fn socket_pref_mismatch_when_dev_pipe_but_prod_requested() {
    assert!(!socket_path_matches_port_pref("domios-dev.sock", false));
}

#[test]
fn is_dev_socket_does_not_match_pid_paths_with_dev_substring() {
    // `contains("dev")` would have classified `domios-99987devops.sock` as dev — guard against that.
    assert!(!is_dev_socket("domios-99987devops.sock"));
    assert!(!is_dev_socket("domios-developerlaptop.sock"));
    assert!(is_dev_socket("domios-dev"));
    assert!(is_dev_socket("domios-dev.sock"));
}

#[test]
fn version_skew_and_missing_method_errors_pass_through_untranslated() {
    for code in [crate::protocol::VERSION_SKEW_CODE, -32601] {
        let data =
            serde_json::json!({"client_protocol":1,"server_protocol":2,"recovery":"rebuild"});
        let result = http_rpc_result(
            reqwest::StatusCode::OK,
            serde_json::json!({"error":{"code":code,"message":"opaque refusal","data":data}}),
        );
        let SocketCallError::Rpc(error) = result.unwrap_err() else {
            panic!("the typed error must stay an RPC error");
        };
        assert_eq!(error.code, code);
        assert_eq!(error.message, "opaque refusal");
        assert_eq!(error.data, Some(data));
    }
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
        credential: "fixture-credential".into(),
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
fn socket_transport_remains_primary_when_available() {
    let choice = select_transport::<()>(Ok(()), || {
        panic!("fallback config must not be read when socket connected")
    });
    assert!(matches!(choice, Ok(TransportChoice::Socket(()))));
}

#[test]
fn credentialed_fallback_skips_health_and_socket_rediscovery() {
    let choice = select_startup_transport::<()>(
        Err("pipe unavailable".into()),
        || http_fallback_config_from(None, Some("7687"), Some("opaque-token")),
        || panic!("credentialed fallback must start with RPC, not health"),
        || panic!("credentialed fallback must not rescan sockets"),
    )
    .unwrap();
    assert!(matches!(choice, TransportChoice::Http(_)));
}

#[test]
fn credentialless_fallback_waits_before_reconnecting_to_ready_socket() {
    let ready = std::cell::Cell::new(false);
    let choice = select_startup_transport(
        Err("pipe unavailable".into()),
        || Err("token is unavailable".into()),
        || {
            ready.set(true);
            Ok(true)
        },
        || {
            assert!(ready.get());
            Ok("ready socket")
        },
    )
    .unwrap();
    assert!(matches!(choice, TransportChoice::Socket("ready socket")));
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
fn startup_discovery_needs_only_a_confined_target_without_control_credentials() {
    assert_eq!(
        http_base_url_from(None, Some("7686")).unwrap(),
        "http://127.0.0.1:7686"
    );
    for target in [
        "http://192.168.1.2:7686",
        "http://127.0.0.1:7686/other",
        "http://user@127.0.0.1:7686",
    ] {
        assert!(http_base_url_from(Some(target), None).is_err());
    }
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

#[test]
fn early_http_rpc_refusal_keeps_starting_phase_through_shared_renderer() {
    let refusal = http_rpc_result(
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        serde_json::json!({
            "ok": false,
            "error": { "code": "starting" },
            "startup": { "state": "starting", "phases": { "providers": "starting" } },
        }),
    )
    .unwrap_err();
    let error = crate::cmd::print::CliError::from(refusal);
    assert_eq!(error.message, "Domios is starting (phase providers)");
    assert_eq!(error.data.as_ref().unwrap()["error"]["code"], "starting");
}

fn restore_env(key: &str, value: Option<String>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

#[test]
fn http_fallback_refuses_an_inherited_url_for_another_instance() {
    let selected = crate::cmd::identify::ResolvedApiConfig {
        data_dir: std::path::PathBuf::from("selected-dev"),
        data_dir_source: ResolutionSource::CodeTermInstance,
        host: "127.0.0.1".into(),
        host_source: ResolutionSource::Default,
        port: 7791,
        port_source: ResolutionSource::Config,
        token: Some("selected-instance-token".into()),
    };
    let error = http_fallback_for_instance(&selected, Some("http://127.0.0.1:7685"))
        .err()
        .expect("foreign URL must refuse fallback");
    assert!(error.contains("differs from the selected instance address"));
    assert!(!error.contains("selected-instance-token"));
    assert_eq!(
        http_fallback_for_instance(&selected, Some("http://127.0.0.1:7791/"))
            .unwrap()
            .base_url,
        "http://127.0.0.1:7791"
    );
    assert_eq!(
        http_fallback_for_instance(&selected, None)
            .unwrap()
            .base_url,
        "http://127.0.0.1:7791"
    );
}

#[test]
fn socket_call_sends_one_request_without_a_version_probe() {
    use interprocess::local_socket::ListenerOptions;

    for code in [None, Some(crate::protocol::VERSION_SKEW_CODE), Some(-32601)] {
        let name = format!("single-rpc-{}-{}", std::process::id(), uuid::Uuid::new_v4());
        let listener = ListenerOptions::new()
            .name(name.as_str().to_ns_name::<GenericNamespaced>().unwrap())
            .create_sync()
            .unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = BufReader::new(listener.accept().unwrap());
            let mut requests = Vec::new();
            loop {
                let mut line = String::new();
                if stream.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                let response = if request["method"] == "system.identify" {
                    serde_json::json!({"result":{"version":"0.0.1"}})
                } else if let Some(code) = code {
                    serde_json::json!({"error":{"code":code,"message":"opaque refusal","data":{"opaque":true}}})
                } else {
                    serde_json::json!({"result":{"opaque":true}})
                };
                writeln!(
                    stream.get_mut(),
                    "{}",
                    serde_json::json!({
                        "jsonrpc":"2.0", "id":request["id"],
                        "result":response.get("result"), "error":response.get("error"),
                    })
                )
                .unwrap();
                requests.push(request);
            }
            requests
        });
        let stream = interprocess::local_socket::Stream::connect(
            name.as_str().to_ns_name::<GenericNamespaced>().unwrap(),
        )
        .unwrap();
        let mut client = SocketClient {
            transport: ClientTransport::Socket(BufReader::new(stream)),
            credential: "opaque-credential".into(),
            next_id: 1,
        };
        let params = serde_json::json!({"tab_id":"opaque-tab"});
        let result = client.call_detailed("tab.list", Some(params.clone()));
        drop(client);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["method"], "tab.list");
        assert_eq!(requests[0]["params"], params);
        assert_eq!(requests[0]["credential"], "opaque-credential");
        assert_eq!(
            requests[0]["protocol_version"],
            crate::protocol::PROTOCOL_VERSION
        );
        if let Some(code) = code {
            let SocketCallError::Rpc(error) = result.unwrap_err() else {
                panic!("RPC error must cross untranslated");
            };
            assert_eq!(error.code, code);
            assert_eq!(error.message, "opaque refusal");
            assert_eq!(error.data, Some(serde_json::json!({"opaque":true})));
        } else {
            assert_eq!(result.unwrap(), serde_json::json!({"opaque":true}));
        }
    }
}
#[test]
fn identity_headers_omit_blank_claims_and_retain_real_values() {
    let _lock = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fields = [
        ("CODETERM_TAB_ID", "x-codeterm-tab-id"),
        ("CODETERM_SESSION_ID", "x-codeterm-session-id"),
        ("CODETERM_SESSION_BINDING_NONCE", "x-codeterm-binding-nonce"),
        ("CODETERM_WORKER_ID", "x-codeterm-worker-id"),
    ];
    let saved = fields.map(|(key, _)| std::env::var_os(key));
    for input in ["", " \t ", "opaque-identity"] {
        for (key, _) in fields {
            std::env::set_var(key, input);
        }
        let headers = super::identity_headers(&fields);
        for (_, header) in fields {
            if input.trim().is_empty() {
                assert!(!headers.contains_key(header));
            } else {
                assert_eq!(headers[header], input);
            }
        }
    }
    for ((key, _), value) in fields.into_iter().zip(saved) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

#[test]
fn isolated_debug_named_socket_accepts_owned_custom_port_without_default_fallback() {
    let display = if cfg!(windows) {
        r"\\.\pipe\domios-dev-verification-a.sock"
    } else {
        "/tmp/domios-dev-verification-a.sock"
    };
    for path in ["domios-dev-verification-a.sock", display] {
        let selected = resolve_socket_selection(Some(path), Some("verification-a"), Some("7798"))
            .unwrap()
            .unwrap();
        assert_eq!(selected.path, "domios-dev-verification-a.sock");
        assert!(
            resolve_socket_selection(Some(path), Some("verification-b"), Some("7798")).is_err()
        );
        assert!(
            resolve_socket_selection(Some(path), Some("verification-a"), Some("7685")).is_err()
        );
        assert!(
            resolve_socket_selection(Some(path), Some("verification-a"), Some("7686")).is_err()
        );
    }
    assert!(resolve_socket_selection(Some("domios-dev.sock"), None, Some("7798")).is_err());
    assert!(resolve_socket_selection(None, None, Some("7798")).is_err());
}

#[test]
fn selected_socket_failure_preserves_os_cause_without_fallback() {
    let fields = io_failure_fields(&std::io::Error::from_raw_os_error(5));
    assert!(fields.contains("os=5"));
    let failure = selected_socket_failure(ResolutionSource::CodeTermSocketPath, &fields);
    assert!(failure.contains("refusing to derive another socket"));
    assert!(failure.ends_with(&fields));
    let private = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "private error body");
    assert_eq!(io_failure_fields(&private), "kind=PermissionDenied os=none");
}

#[test]
fn http_failure_diagnostic_redacts_request_and_error_strings() {
    let error = reqwest::blocking::Client::new()
        .get("https://private-user:private-password@private.invalid:invalid-port/private-path?token=private-token")
        .build().unwrap_err();
    let message = http_failure_message("http_probe_send", &error);
    assert_eq!(message, "loopback HTTP failure stage=http_probe_send class=builder kind=none os=none");
    assert!(!message.contains("private"));
}
#[test]
fn malformed_socket_connect_retains_finite_failure_fields() {
    let failure = SocketClient::connect("invalid\0socket").err().unwrap();
    assert!(failure.contains("stage=socket_connect"));
    assert!(failure.contains("kind="));
    assert!(failure.contains("os="));
}
