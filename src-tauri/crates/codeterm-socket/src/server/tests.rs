use super::*;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct FailingWriter {
    kind: io::ErrorKind,
}

impl Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(self.kind, "test disconnect"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn test_response() -> JsonRpcResponse {
    JsonRpcResponse::success(Some(serde_json::json!(1)), serde_json::json!({"ok": true}))
}

#[test]
fn disconnected_response_write_closes_connection_without_error() {
    for kind in [
        io::ErrorKind::BrokenPipe,
        io::ErrorKind::ConnectionReset,
        io::ErrorKind::ConnectionAborted,
        io::ErrorKind::NotConnected,
        io::ErrorKind::UnexpectedEof,
    ] {
        let mut writer = FailingWriter { kind };
        let keep_open = write_json_rpc_response(&mut writer, &test_response()).unwrap();
        assert!(
            !keep_open,
            "{kind:?} should close only the client connection"
        );
    }
}

struct FlushProbe<F> {
    input: io::Cursor<Vec<u8>>,
    output: Vec<u8>,
    is_tab_open: F,
    tab_open_at_flush: Option<bool>,
}

impl<F: Fn() -> bool> Read for FlushProbe<F> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.input.read(buf)
    }
}

impl<F: Fn() -> bool> Write for FlushProbe<F> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tab_open_at_flush = Some((self.is_tab_open)());
        Ok(())
    }
}

#[test]
fn cron_settle_cron_complete_closes_after_response_flush() {
    for (replacement, status) in [(false, None), (true, None), (false, Some("failed"))] {
        use codeterm_service_core::cron::store::{
            CronCreateParams, CronRunInfo, CronRunStatus, CronStore,
        };
        use std::sync::Arc;

        let (mut mux, initial_tab) =
            codeterm_service_core::multiplexer::state::MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&initial_tab).unwrap();
        let tab_id = mux.create_tab(&workspace).unwrap();
        let fixture = codeterm_neighbors_service::test_support::fixture(mux);
        let mut store = CronStore::new_with_path(fixture.temp.path().join("crons.json"));
        let job_id = store
            .create(CronCreateParams {
                name: "rpc completion".into(),
                workspace_id: "workspace".into(),
                team_id: None,
                detach_from_team: false,
                schedule: "0 * * * *".into(),
                task: "complete run".into(),
                description: None,
                agent: None,
                model: None,
                reasoning_effort: None,
                cwd: None,
                role: None,
                skill: None,
                creator_tab_id: None,
                report_target_tab_id: None,
                enabled: Some(false),
                max_history: None,
                oneshot: Some(false),
                jitter_max_ms: Some(0),
                tz: None,
                timeout_seconds: None,
            })
            .unwrap()
            .id;
        store
            .record_run(
                &job_id,
                CronRunInfo {
                    scheduled_occurrence_at: None,
                    fired_at: None,
                    late_by_ms: None,
                    provider: None,
                    owner_instance_id: None,
                    cancel_reason: None,
                    run_id: "rpc-complete-run".into(),
                    started_at: 1,
                    finished_at: None,
                    status: CronRunStatus::Running,
                    tab_id: Some(tab_id.clone()),
                    session_id: None,
                    cwd: None,
                    summary: None,
                    report_delivery: None,
                },
            )
            .unwrap();

        let cron: codeterm_service_core::CronState = Arc::new(parking_lot::Mutex::new(store));
        let wake = codeterm_service_core::CronWake::default();
        assert!(fixture.service.set_ext(cron.clone()).is_ok());
        assert!(fixture
            .service
            .set_ext::<codeterm_service_core::CronWake>(wake)
            .is_ok());

        fixture
            .service
            .prepare_runtime_cron_owner_for_test(&tab_id, &job_id, "rpc-complete-run")
            .unwrap();
        fixture
            .service
            .ext::<codeterm_service_core::config::ConfigState>()
            .lock()
            .api
            .token = Some("flush-owner-credential".into());
        let mut request = serde_json::json!({
            "jsonrpc": "2.0",
            "credential": "flush-owner-credential",
            "id": 1,
            "protocol_version": protocol::PROTOCOL_VERSION,
            "method": "cron.complete",
            "params": {
                "run_id": "rpc-complete-run",
                "summary": "response-flush-summary",
                "human_confirmed": true
            }
        });
        if let Some(status) = status {
            request["params"]["status"] = status.into();
        }
        let input = format!("{}\n", serde_json::to_string(&request).unwrap()).into_bytes();
        let mux = fixture.service.mux.clone();
        let flush_mux = mux.clone();
        let flush_tab = tab_id.clone();
        let mut stream = FlushProbe {
            input: io::Cursor::new(input),
            output: Vec::new(),
            is_tab_open: move || {
                let mut mux = flush_mux.write();
                let open = mux.get_tab(&flush_tab).is_some();
                if replacement {
                    let tab = mux.get_tab_mut(&flush_tab).unwrap();
                    tab.session_binding_nonce = Some("replacement-after-response".into());
                    tab.pty_id = Some("replacement-pty".into());
                }
                open
            },
            tab_open_at_flush: None,
        };
        let svc = Arc::new(fixture.service.clone());

        if status == Some("failed") {
            let mut invalid = request["params"].clone();
            invalid["status"] = "running".into();
            assert_eq!(
                super::cron::dispatch_complete(&Some(invalid), &svc)
                    .err()
                    .unwrap()
                    .code,
                -32602
            );
            assert_eq!(
                cron.lock().get_history(&job_id)[0].status,
                CronRunStatus::Running
            );
        }
        let (ended, settled) = mpsc::channel();
        fixture.ui.listen("cron-run-finished", move |_| {
            let _ = ended.send(());
        });
        handle_connection(
            &mut stream,
            &svc.mux,
            &svc.pty,
            &svc.notif,
            &svc.output_buf,
            &svc,
        );

        assert_eq!(stream.tab_open_at_flush, Some(true));
        let response: serde_json::Value =
            serde_json::from_slice(stream.output.strip_suffix(b"\n").unwrap()).unwrap();
        assert_eq!(response["result"]["ok"], true, "response: {response}");
        if replacement {
            let mux = mux.read();
            let tab = mux.get_tab(&tab_id).unwrap();
            assert_eq!(
                tab.session_binding_nonce.as_deref(),
                Some("replacement-after-response")
            );
            assert_eq!(tab.pty_id.as_deref(), Some("replacement-pty"));
        } else {
            assert!(mux.read().get_tab(&tab_id).is_none());
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while cron
            .lock()
            .retained_run_report(&job_id, "rpc-complete-run")
            .is_none()
        {
            settled
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .unwrap();
        }
        let receipt = cron.lock().get_history(&job_id)[0].clone();
        let expected = if status == Some("failed") {
            CronRunStatus::Failed
        } else {
            CronRunStatus::Success
        };
        assert_eq!(receipt.status, expected);
        let store = cron.lock();
        let job = store.get(&job_id).unwrap();
        assert_eq!(job.last_run.as_ref().unwrap().status, expected);
        assert_eq!(
            job.consecutive_failures,
            u32::from(expected == CronRunStatus::Failed)
        );
        assert_eq!(receipt.summary.as_deref(), Some("response-flush-summary"));
        assert_eq!(receipt.report_delivery.as_deref(), Some("no report target"));
    }
}

#[test]
fn log_composition_classifies_expected_socket_disconnects_without_error() {
    for kind in [
        io::ErrorKind::BrokenPipe,
        io::ErrorKind::ConnectionReset,
        io::ErrorKind::ConnectionAborted,
        io::ErrorKind::NotConnected,
        io::ErrorKind::UnexpectedEof,
    ] {
        assert!(is_disconnected_client(&io::Error::from(kind)));
    }
    assert!(!is_disconnected_client(&io::Error::from(
        io::ErrorKind::PermissionDenied
    )));
}

#[test]
fn version_skew_socket_gate_reads_the_request_envelope() {
    let stale: JsonRpcRequest = serde_json::from_str(
        r#"{"jsonrpc":"2.0","id":7,"method":"tab.list","protocol_version":"not-an-integer"}"#,
    )
    .expect("a malformed version field must not fail request parsing");
    let error = protocol::version_skew_gate(
        protocol::client_protocol_version(stale.protocol_version.as_ref()),
        protocol::PROTOCOL_VERSION,
        protocol::RECOVERY_COMMAND,
    )
    .expect("oldest client is refused with the typed error");
    assert_eq!(error.code, protocol::VERSION_SKEW);
    let data = error.data.expect("typed error carries structured data");
    assert_eq!(data["client_protocol"], 0);
    assert_eq!(data["server_protocol"], protocol::PROTOCOL_VERSION);

    let absent: JsonRpcRequest =
        serde_json::from_str(r#"{"jsonrpc":"2.0","id":8,"method":"tab.list"}"#)
            .expect("an absent version field must not fail request parsing");
    assert_eq!(
        protocol::client_protocol_version(absent.protocol_version.as_ref()),
        0
    );
}

#[test]
fn version_skew_same_version_request_passes_the_gate_unchanged() {
    let request = format!(
        r#"{{"jsonrpc":"2.0","id":9,"method":"tab.list","protocol_version":{}}}"#,
        protocol::PROTOCOL_VERSION
    );
    let current: JsonRpcRequest = serde_json::from_str(&request).unwrap();
    assert!(protocol::version_skew_gate(
        protocol::client_protocol_version(current.protocol_version.as_ref()),
        protocol::PROTOCOL_VERSION,
        protocol::RECOVERY_COMMAND,
    )
    .is_none());
}

#[test]
fn normal_response_write_stays_open() {
    let mut out = Vec::new();
    let keep_open = write_json_rpc_response(&mut out, &test_response()).unwrap();
    assert!(keep_open);
    let line = std::str::from_utf8(&out).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains("\"result\":{\"ok\":true}"));
}

#[test]
fn local_socket_response_write_survives_client_disconnect() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let socket_name = format!(
        "{}-disconnect-repro-{}-{}.sock",
        codeterm_service_core::paths::socket_prefix(),
        std::process::id(),
        nonce
    );

    let server_name = socket_name
        .clone()
        .to_ns_name::<GenericNamespaced>()
        .unwrap();
    let listener = ListenerOptions::new()
        .name(server_name)
        .create_sync()
        .unwrap();
    let client_name = socket_name
        .clone()
        .to_ns_name::<GenericNamespaced>()
        .unwrap();
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    let server = std::thread::spawn(move || {
        let mut stream = listener.incoming().next().unwrap().unwrap();
        accepted_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        write_json_rpc_response(&mut stream, &test_response())
    });

    let client = interprocess::local_socket::Stream::connect(client_name).unwrap();
    accepted_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(client);
    release_tx.send(()).unwrap();

    let keep_open = server.join().unwrap().unwrap();
    assert!(
        !keep_open,
        "disconnected local-socket client must end only its connection"
    );

    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::fs::remove_file(format!("/tmp/{socket_name}"));
    }
}

#[cfg(target_os = "windows")]
#[test]
fn windows_blocking_pipe_read_survives_client_disconnect() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let socket_name = format!(
        "{}-read-disconnect-repro-{}-{}.sock",
        codeterm_service_core::paths::socket_prefix(),
        std::process::id(),
        nonce
    );

    let server_name = socket_name
        .clone()
        .to_ns_name::<GenericNamespaced>()
        .unwrap();
    let listener = ListenerOptions::new()
        .name(server_name)
        .create_sync()
        .unwrap();
    let client_name = socket_name.to_ns_name::<GenericNamespaced>().unwrap();
    let (accepted_tx, accepted_rx) = mpsc::channel();

    let server = std::thread::spawn(move || {
        let stream = listener.incoming().next().unwrap().unwrap();
        let mut reader = BufReader::new(WindowsBlockingPipe::from(stream));
        accepted_tx.send(()).unwrap();
        let mut line = String::new();
        reader.read_line(&mut line)
    });

    let client = interprocess::local_socket::Stream::connect(client_name).unwrap();
    accepted_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(client);

    let result = server.join().unwrap();
    if let Err(e) = result {
        assert!(
            is_disconnected_client(&e),
            "disconnect read should not surface as fatal socket error: {e}"
        );
    }
}

#[cfg(target_os = "windows")]
#[test]
fn windows_blocking_pipe_round_trips_delayed_json_rpc() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let socket_name = format!(
        "{}-read-write-roundtrip-{}-{}.sock",
        codeterm_service_core::paths::socket_prefix(),
        std::process::id(),
        nonce
    );

    let server_name = socket_name
        .clone()
        .to_ns_name::<GenericNamespaced>()
        .unwrap();
    let listener = ListenerOptions::new()
        .name(server_name)
        .create_sync()
        .unwrap();
    let client_name = socket_name.to_ns_name::<GenericNamespaced>().unwrap();
    let (accepted_tx, accepted_rx) = mpsc::channel();

    let server = std::thread::spawn(move || {
        let stream = listener.incoming().next().unwrap().unwrap();
        let mut pipe = WindowsBlockingPipe::from(stream);
        accepted_tx.send(()).unwrap();

        let mut line = String::new();
        {
            let mut reader = BufReader::new(&mut pipe);
            reader.read_line(&mut line).unwrap();
        }

        assert!(line.contains("\"method\":\"socket.roundtrip\""));
        write_json_rpc_response(&mut pipe, &test_response()).unwrap();
    });

    let mut client = interprocess::local_socket::Stream::connect(client_name).unwrap();
    accepted_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    client
        .write_all(br#"{"jsonrpc":"2.0","id":1,"method":"socket.roundtrip"}"#)
        .unwrap();
    client.write_all(b"\n").unwrap();
    client.flush().unwrap();

    let mut response = String::new();
    BufReader::new(client).read_line(&mut response).unwrap();
    server.join().unwrap();

    assert!(response.contains("\"id\":1"));
    assert!(response.contains("\"ok\":true"));
}
