use super::*;
use codeterm_service_core::agent::test_provider::MockProvider;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use std::cell::RefCell;
use std::sync::{Arc, Once};

thread_local! {
    static LINES: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

struct CaptureLogger;

impl log::Log for CaptureLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        LINES.with(|lines| {
            if let Some(lines) = lines.borrow_mut().as_mut() {
                let line = record.args().to_string();
                if line.starts_with("[socket-request]")
                    || line.starts_with("[boot-phase]")
                    || line.starts_with("[boot-request]")
                {
                    assert_eq!(record.level(), log::Level::Debug);
                    lines.push(line);
                }
            }
        });
    }

    fn flush(&self) {}
}

struct Capture;

impl Capture {
    fn start() -> Self {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            log::set_logger(&CaptureLogger).unwrap();
            log::set_max_level(log::LevelFilter::Debug);
        });
        LINES.with(|lines| assert!(lines.replace(Some(Vec::new())).is_none()));
        Self
    }

    fn lines(&self) -> Vec<String> {
        LINES.with(|lines| lines.borrow().as_ref().unwrap().clone())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        LINES.with(|lines| lines.replace(None));
    }
}

struct Stream {
    input: io::Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.input.read(buf)
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn socket_request(fixture: &ServiceFixture, mut request: serde_json::Value) -> serde_json::Value {
    fixture.service.ext::<codeterm_service_core::config::ConfigState>().lock().api.token = Some("arrival-owner-credential".into());
    request["credential"] = serde_json::json!("arrival-owner-credential");
    let mut stream = Stream {
        input: io::Cursor::new(format!("{request}\n").into_bytes()),
        output: Vec::new(),
    };
    let svc = Arc::new(fixture.service.clone());
    handle_connection(
        &mut stream,
        &svc.mux,
        &svc.pty,
        &svc.notif,
        &svc.output_buf,
        &svc,
    );
    serde_json::from_slice(&stream.output).unwrap()
}

#[test]
fn socket_agent_spawn_binds_request_before_rejected_launch() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_empty_launch_command_when_task_present("arrival-fixture"),
    ));
    let capture = Capture::start();
    let response = socket_request(
        &fixture,
        serde_json::json!({
            "jsonrpc": "2.0", "id": "spawn-request", "method": "agent.spawn",
            "protocol_version": protocol::PROTOCOL_VERSION,
            "params": {"agent": "arrival-fixture", "task": "fixture-input"},
        }),
    );
    assert_eq!(response["id"], "spawn-request");
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("did not resolve a launch command"));
    let lines = capture.lines();
    let binding = lines
        .iter()
        .find(|line| line.starts_with("[boot-request]"))
        .unwrap();
    let tab_id = binding
        .split_whitespace()
        .nth(1)
        .unwrap()
        .strip_prefix("tab=")
        .unwrap();
    assert!(binding.contains("request_id=\"spawn-request\" method=agent.spawn "));
    for phase in ["request_received", "request_dispatched"] {
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.starts_with(&format!(
                    "[boot-phase] tab={tab_id} pty=- phase=\"{phase}\" "
                )))
                .count(),
            1
        );
    }
    let before = capture.lines();
    boot_timing::bind_request("internal-after-socket");
    assert_eq!(capture.lines(), before);
}

#[test]
fn spawn_cli_request_marks_precede_pty_starting_without_starting_a_pty() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_empty_launch_command("arrival-fixture"),
    ));
    let clearance = codeterm_service_core::service::spawn::provider_launch_preflight(
        &fixture.service.agent_registry,
        "arrival-fixture",
        false,
    )
    .unwrap();
    let capture = Capture::start();
    let tab_id = {
        let _context = boot_timing::enter_request(Some(&serde_json::json!(37)), "agent.spawn", 101);
        let mut mux = fixture.service.mux.write();
        let workspace = mux.list_workspaces()[0].id.clone();
        codeterm_service_core::service::spawn::create_cleared_agent_tab(
            &mut mux,
            &workspace,
            &clearance,
            &codeterm_service_core::service::billing::LimitGate::dormant(),
        )
        .unwrap()
    };
    fixture.service.emit_boot_stage(
        &tab_id,
        codeterm_service_core::service::boot_stage::BootStage::PtyStarting,
        None,
    );
    let lines = capture.lines();
    let received = lines
        .iter()
        .position(|line| line.contains("phase=\"request_received\""))
        .unwrap();
    let dispatched = lines
        .iter()
        .position(|line| line.contains("phase=\"request_dispatched\""))
        .unwrap();
    let bound = lines
        .iter()
        .position(|line| line.starts_with("[boot-request]"))
        .unwrap();
    let starting = lines
        .iter()
        .position(|line| line.contains("phase=PtyStarting "))
        .unwrap();
    assert!(received < dispatched && dispatched < bound && bound < starting);
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert!(tab.pty_id.is_none());
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_service_core::service::boot_stage::BootStage::PtyStarting)
    );
}

#[test]
fn socket_tab_new_binds_once_and_internal_tab_is_silent() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let capture = Capture::start();
    let response = socket_request(
        &fixture,
        serde_json::json!({
            "jsonrpc": "2.0", "id": 9, "method": "tab.new",
            "protocol_version": protocol::PROTOCOL_VERSION, "params": {},
        }),
    );
    let tab_id = response["result"]["tab_id"].as_str().unwrap();
    assert!(fixture.service.mux.read().get_tab(tab_id).is_some());
    let lines = capture.lines();
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("phase=\"request_received\""))
            .count(),
        1
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("phase=\"request_dispatched\""))
            .count(),
        1
    );
    assert!(lines.iter().any(|line| line.starts_with(&format!(
        "[boot-request] tab={tab_id} request_id=9 method=tab.new "
    ))));
    fixture.service.spawn_tab(Default::default()).unwrap();
    assert_eq!(capture.lines(), lines);
}

#[test]
fn invalid_socket_request_logs_arrival_without_installing_context() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let capture = Capture::start();
    let response = socket_request(&fixture, serde_json::json!({}));
    assert_eq!(response["error"]["code"], -32700);
    let lines = capture.lines();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("[socket-request] id=null method=invalid received_at_us="));
    boot_timing::bind_request("internal-after-invalid-request");
    assert_eq!(capture.lines(), lines);
}
