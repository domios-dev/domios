//! JSON-RPC v2 server over named pipe (Windows) / Unix socket.
//!
//! Outer dispatch routes by method-prefix to per-domain submodules. Children
//! own only the state they actually need (workspace + tab carry MuxState +
//! AppHandle, notification gets NotifState, cron/agent/bridge get ServiceState
//! + AppHandle).

mod agent;
mod agent_commands;
mod bridge;
mod canvas;
mod cron;
mod dispatch;
mod mem;
mod notification;
mod orchestration;
mod orchestrator;
mod tab;
mod plan;
mod plan_archive;
mod plan_attachments;
mod plan_competition;
mod plan_elements;
mod plan_fields;
mod plan_gates;
mod plan_goal;
mod plan_handoff;
mod plan_heartbeat;
mod plan_lane_fields;
mod plan_lifecycle;
mod plan_linking;
mod plan_loops;
mod plan_mode;
mod plan_qa;
mod plan_read;
mod plan_refs;
mod plan_review;
mod plan_role_helpers;
mod plan_skills_rpc;
mod plan_task_dispatch;
mod plan_task_rpc;
mod plan_time;
mod services;
mod spec;
mod team;
mod workspace;

pub(crate) use dispatch::dispatch_service_rpc;

use std::io::{self, BufRead, BufReader, Read, Write};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions};

#[cfg(target_os = "windows")]
use std::os::windows::io::{AsRawHandle, OwnedHandle, RawHandle};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_IO_PENDING, HANDLE};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Storage::FileSystem::{FlushFileBuffers, ReadFile, WriteFile};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::CreateEventW;
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};

use crate::multiplexer::commands::MuxState;
use crate::notification::NotifState;
use crate::service::ServiceState;
use crate::{OutputBufferState, PtyState};

// ── JSON-RPC types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct JsonRpcError {
    pub(crate) code: i32,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<serde_json::Value>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
        }
    }
}

// ── Socket path ─────────────────────────────────────────────────────

/// Get the namespaced socket name for the current process.
///
/// `GenericNamespaced` on Unix prepends `/tmp/` automatically,
/// so we only provide the bare name here.
pub fn socket_path() -> String {
    // time. Prod builds include PID to allow multiple instances.
    #[cfg(debug_assertions)]
    {
        format!("{}.sock", crate::paths::socket_prefix())
    }
    #[cfg(not(debug_assertions))]
    {
        let pid = std::process::id();
        format!("{}-{}.sock", crate::paths::socket_prefix(), pid)
    }
}

/// Get the full display path (for env var / user display).
///
/// On Unix, `GenericNamespaced` places the socket at `/tmp/<name>`,
/// so the display path is `/tmp/{prefix}-{pid}.sock`.
/// On Windows, it's `\\.\pipe\{prefix}-{pid}`.
pub fn socket_display_path() -> String {
    let bare = socket_path();

    #[cfg(target_os = "windows")]
    {
        format!("\\\\.\\pipe\\{}", bare)
    }

    #[cfg(not(target_os = "windows"))]
    {
        format!("/tmp/{}", bare)
    }
}

// ── Server ──────────────────────────────────────────────────────────

pub struct SocketServer;

impl SocketServer {
    /// Clean up stale sockets from previous runs that didn't exit cleanly.
    pub fn cleanup_stale_sockets() {
        #[cfg(not(target_os = "windows"))]
        {
            if let Ok(entries) = std::fs::read_dir("/tmp") {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        let sock_prefix = format!("{}-", crate::paths::socket_prefix());
                        if name.starts_with(&sock_prefix) && name.ends_with(".sock") {
                            if let Some(pid_str) = name
                                .strip_prefix(&sock_prefix)
                                .and_then(|s| s.strip_suffix(".sock"))
                            {
                                if let Ok(pid) = pid_str.parse::<u32>() {
                                    let alive = std::process::Command::new("kill")
                                        .args(["-0", &pid.to_string()])
                                        .stdout(std::process::Stdio::null())
                                        .stderr(std::process::Stdio::null())
                                        .status()
                                        .map(|s| s.success())
                                        .unwrap_or(false);
                                    if !alive {
                                        let _ = std::fs::remove_file(&path);
                                        log::info!("Cleaned stale socket: {}", name);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Start the JSON-RPC server in a background thread.
    pub fn start(
        mux_state: MuxState,
        pty_state: PtyState,
        notif_state: NotifState,
        app: AppHandle,
        output_buf: OutputBufferState,
        svc_state: ServiceState,
    ) -> Result<(), String> {
        Self::cleanup_stale_sockets();

        #[cfg(all(debug_assertions, not(target_os = "windows")))]
        {
            let display = socket_display_path();
            let path = std::path::Path::new(&display);
            if path.exists() {
                let _ = std::fs::remove_file(path);
                log::info!("Removed stale dev socket: {}", display);
            }
        }

        let name = socket_path();

        let name = name
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| format!("Failed to create namespaced name: {}", e))?;

        let opts = ListenerOptions::new().name(name);

        let listener = opts
            .create_sync()
            .map_err(|e| format!("Failed to create socket listener: {}", e))?;

        log::info!("Socket server listening at {}", socket_display_path());

        std::thread::spawn(move || {
            for conn in listener.incoming() {
                match conn {
                    Ok(stream) => {
                        #[cfg(target_os = "windows")]
                        let stream = WindowsBlockingPipe::from(stream);

                        let mux = mux_state.clone();
                        let pty = pty_state.clone();
                        let notif = notif_state.clone();
                        let app_handle = app.clone();
                        let out_buf = output_buf.clone();
                        let svc = svc_state.clone();

                        std::thread::spawn(move || {
                            handle_connection(
                                stream,
                                &mux,
                                &pty,
                                &notif,
                                &app_handle,
                                &out_buf,
                                &svc,
                            );
                        });
                    }
                    Err(e) => {
                        log::error!("Socket accept error: {}", e);
                    }
                }
            }
        });

        Ok(())
    }
}

#[cfg(target_os = "windows")]
struct WindowsBlockingPipe {
    handle: OwnedHandle,
}

#[cfg(target_os = "windows")]
struct WindowsEvent(HANDLE);

#[cfg(target_os = "windows")]
impl WindowsEvent {
    fn new() -> io::Result<Self> {
        let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsEvent {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl From<interprocess::local_socket::Stream> for WindowsBlockingPipe {
    fn from(stream: interprocess::local_socket::Stream) -> Self {
        let interprocess::local_socket::Stream::NamedPipe(stream) = stream;
        Self {
            handle: stream.into(),
        }
    }
}

#[cfg(target_os = "windows")]
impl Read for WindowsBlockingPipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        run_overlapped_io(self.handle.as_raw_handle(), |overlapped| unsafe {
            ReadFile(
                self.handle.as_raw_handle(),
                buf.as_mut_ptr().cast(),
                len,
                std::ptr::null_mut(),
                overlapped,
            )
        })
    }
}

#[cfg(target_os = "windows")]
impl Write for WindowsBlockingPipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        run_overlapped_io(self.handle.as_raw_handle(), |overlapped| unsafe {
            WriteFile(
                self.handle.as_raw_handle(),
                buf.as_ptr().cast(),
                len,
                std::ptr::null_mut(),
                overlapped,
            )
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        let ok = unsafe { FlushFileBuffers(self.handle.as_raw_handle()) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(target_os = "windows")]
fn run_overlapped_io(
    handle: RawHandle,
    issue: impl FnOnce(*mut OVERLAPPED) -> i32,
) -> io::Result<usize> {
    let event = WindowsEvent::new()?;
    let mut overlapped = OVERLAPPED::default();
    overlapped.hEvent = event.0;

    let ok = issue(&mut overlapped);
    if ok == 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
            return Err(err);
        }
    }

    let mut transferred = 0_u32;
    let ok = unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, 1) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(transferred as usize)
    }
}

/// Handle a single client connection: read lines, parse JSON-RPC, respond.
fn handle_connection(
    stream: impl Read + Write,
    mux: &MuxState,
    pty: &PtyState,
    notif: &NotifState,
    app: &AppHandle,
    output_buf: &OutputBufferState,
    svc: &ServiceState,
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                    Ok(req) => {
                        let id = req.id.clone();
                        let id_for_panic = id.clone();
                        let method = req.method.clone();
                        // catch_unwind so a panic inside any dispatch handler does not silently drop the socket and leave the CLI with an "EOF while parsing" mystery.
                        // We surface a real JSON-RPC error instead.
                        let dispatched =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                dispatch::dispatch(
                                    &req.method,
                                    req.params,
                                    mux,
                                    pty,
                                    notif,
                                    app,
                                    output_buf,
                                    svc,
                                )
                            }));
                        match dispatched {
                            Ok(Ok(result)) => JsonRpcResponse::success(id, result),
                            Ok(Err(err)) => JsonRpcResponse {
                                jsonrpc: "2.0".to_string(),
                                id,
                                result: None,
                                error: Some(err),
                            },
                            Err(panic) => {
                                let panic_msg = if let Some(s) = panic.downcast_ref::<&str>() {
                                    (*s).to_string()
                                } else if let Some(s) = panic.downcast_ref::<String>() {
                                    s.clone()
                                } else {
                                    "non-string panic payload".to_string()
                                };
                                log::error!(
                                    "JSON-RPC dispatch panicked for method '{}': {}",
                                    method,
                                    panic_msg
                                );
                                JsonRpcResponse::error(
                                    id_for_panic,
                                    -32603,
                                    format!("internal panic in '{}': {}", method, panic_msg),
                                )
                            }
                        }
                    }
                    Err(e) => JsonRpcResponse::error(None, -32700, format!("Parse error: {}", e)),
                };

                if let Ok(keep_open) = write_json_rpc_response(reader.get_mut(), &response) {
                    if !keep_open {
                        break;
                    }
                }
            }
            Err(e) => {
                log::error!("Socket read error: {}", e);
                break;
            }
        }
    }
}

fn write_json_rpc_response(
    writer: &mut impl Write,
    response: &JsonRpcResponse,
) -> io::Result<bool> {
    let json = serde_json::to_string(response).map_err(io::Error::other)?;
    match write_response_line(writer, json.as_bytes()) {
        Ok(()) => Ok(true),
        Err(e) if is_disconnected_client(&e) => {
            log::debug!("Socket client disconnected before response write: {}", e);
            Ok(false)
        }
        Err(e) => {
            log::error!("Socket response write error: {}", e);
            Err(e)
        }
    }
}

fn write_response_line(writer: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    writer.write_all(bytes)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn is_disconnected_client(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::NotConnected
            | io::ErrorKind::UnexpectedEof
    )
}

/// Extract a required string parameter from JSON-RPC params.
pub(super) fn opt_string_param(params: &Option<serde_json::Value>, key: &str) -> Option<String> {
    params
        .as_ref()
        .and_then(|p| p.get(key))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

pub(super) fn get_string_param(
    params: &Option<serde_json::Value>,
    key: &str,
) -> Result<String, JsonRpcError> {
    params
        .as_ref()
        .and_then(|p| p.get(key))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| JsonRpcError {
            code: -32602,
            message: format!("Missing required parameter: {}", key),
            data: None,
        })
}

#[cfg(test)]
mod tests {
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
            crate::paths::socket_prefix(),
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

        let server = std::thread::spawn(move || {
            let mut stream = listener.incoming().next().unwrap().unwrap();
            accepted_tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            write_json_rpc_response(&mut stream, &test_response())
        });

        let client = interprocess::local_socket::Stream::connect(client_name).unwrap();
        accepted_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(client);

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
            crate::paths::socket_prefix(),
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
            crate::paths::socket_prefix(),
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
        std::thread::sleep(Duration::from_millis(50));
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
}
