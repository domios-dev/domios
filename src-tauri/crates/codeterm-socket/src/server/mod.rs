//! JSON-RPC v2 server over named pipe (Windows) / Unix socket.
use codeterm_neighbors_service::prelude::*;

mod agent;
mod agent_commands;
mod blueprint;
mod bridge;
mod canvas;
#[cfg(test)]
mod credential_tests;
mod cron;
mod dispatch;
mod error_codes;
mod local_api;
mod mem;
mod model_errors;
#[cfg(test)]
mod model_provider_authority_tests;
mod model_providers;
mod notification;
mod orchestration;
mod orchestrator;
pub(crate) mod plan;
mod plan_archive;
mod plan_attachments;
#[cfg(test)]
mod plan_batch_authority_tests;
mod plan_competition;
mod plan_curator;
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
mod plan_task_authority;
pub(crate) mod plan_task_dispatch;
mod plan_task_rpc;
mod plan_time;
pub(crate) mod protocol;
mod radar;
mod receiver_target;
mod remote_team_rpc;
mod remote_worker_policy;
#[cfg(test)]
mod request_arrival_tests;
pub(crate) mod rest_api;
mod role;
mod safety_nudge;
mod services;
mod spec;
mod tab;
mod tab_todos;
mod team;
mod team_exchange;
mod team_template_start;
mod workspace;

pub(crate) use dispatch::{dispatch_mesh_rpc, dispatch_service_rpc};

#[cfg(test)]
mod neighbor_tests;

use std::io::{self, BufRead, BufReader, Read, Write};

use serde::{Deserialize, Serialize};

use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions};

#[cfg(target_os = "linux")]
use interprocess::os::unix::local_socket::ListenerOptionsExt as UnixListenerOptionsExt;

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

use codeterm_service_core::multiplexer::commands::MuxState;
use codeterm_service_core::notification::NotifState;
use codeterm_service_core::service::ServiceState;
use codeterm_service_core::startup::boot_timing;
use codeterm_service_core::{OutputBufferState, PtyState};

// ── JSON-RPC types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    credential: String,
    params: Option<serde_json::Value>,
    /// Lenient by design: any non-integer value gates as the oldest protocol.
    #[serde(default)]
    protocol_version: Option<serde_json::Value>,
}

type PostResponseAction = Box<dyn FnOnce() + Send>;

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
    #[serde(skip)]
    after_response: Option<PostResponseAction>,
}

pub(crate) use codeterm_service_core::ports::socket::RpcError as JsonRpcError;

impl JsonRpcResponse {
    fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
            after_response: None,
        }
    }

    fn error(id: Option<serde_json::Value>, code: i32, message: String) -> Self {
        let data = (code == -32003 && message.starts_with("neighbor_capability_denied:"))
            .then(|| serde_json::json!({"reason":"neighbor_capability_denied"}));
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data,
            }),
            after_response: None,
        }
    }
}

// ── Server ──────────────────────────────────────────────────────────

pub struct SocketServer;

impl SocketServer {
    /// Start the JSON-RPC server in a background thread.
    pub fn start(
        mux_state: MuxState,
        pty_state: PtyState,
        notif_state: NotifState,
        output_buf: OutputBufferState,
        svc_state: ServiceState,
    ) -> Result<(), String> {
        let name = codeterm_service_core::paths::socket_path();

        codeterm_util::local_endpoint::prepare_namespaced(&name)
            .map_err(|error| error.to_string())?;
        let target = codeterm_util::local_endpoint::display_name(&name);
        let name = name
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| codeterm_util::local_endpoint::bind_error(&target, e).to_string())?;

        let opts = ListenerOptions::new().name(name);

        #[cfg(target_os = "linux")]
        let opts = opts.mode(0o600);

        let listener = opts
            .create_sync()
            .map_err(|e| codeterm_util::local_endpoint::bind_error(&target, e).to_string())?;

        log::info!(
            "Socket server listening at {}",
            codeterm_service_core::paths::socket_display_path()
        );

        codeterm_util::named_thread::spawn("socket-listener", move || {
            for conn in listener.incoming() {
                match conn {
                    Ok(stream) => {
                        if !peer_is_current_user(&stream) {
                            log::warn!("Rejected local socket client owned by another OS user");
                            continue;
                        }

                        #[cfg(target_os = "windows")]
                        let stream = WindowsBlockingPipe::from(stream);

                        let mux = mux_state.clone();
                        let pty = pty_state.clone();
                        let notif = notif_state.clone();
                        let out_buf = output_buf.clone();
                        let svc = svc_state.clone();

                        codeterm_util::named_thread::spawn("socket-connection", move || {
                            handle_connection(stream, &mux, &pty, &notif, &out_buf, &svc);
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

#[cfg(unix)]
fn peer_is_current_user(stream: &interprocess::local_socket::Stream) -> bool {
    stream
        .peer_creds()
        .ok()
        .and_then(|credentials| credentials.euid())
        .is_some_and(|euid| euid == unsafe { libc::geteuid() })
}

#[cfg(target_os = "windows")]
fn peer_is_current_user(stream: &interprocess::local_socket::Stream) -> bool {
    let peer_pid = match stream
        .peer_creds()
        .ok()
        .and_then(|credentials| credentials.pid())
    {
        Some(pid) => pid,
        None => return false,
    };
    windows_process_user_sid(Some(peer_pid))
        .and_then(|peer| windows_process_user_sid(None).map(|current| (peer, current)))
        .is_ok_and(|(peer, current)| unsafe {
            windows_sys::Win32::Security::EqualSid(
                peer.as_ptr().cast_mut().cast(),
                current.as_ptr().cast_mut().cast(),
            ) != 0
        })
}

#[cfg(target_os = "windows")]
fn windows_process_user_sid(pid: Option<u32>) -> io::Result<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER},
        System::Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };

    let process = match pid {
        Some(pid) => unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) },
        None => unsafe { GetCurrentProcess() },
    };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }

    let mut token: HANDLE = std::ptr::null_mut();
    let opened = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) };
    if pid.is_some() {
        unsafe { CloseHandle(process) };
    }
    if opened == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut size = 0;
    unsafe {
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size);
    }
    let mut token_info = vec![0u8; size as usize];
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            token_info.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    };
    unsafe { CloseHandle(token) };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }

    let user = unsafe { &*(token_info.as_ptr().cast::<TOKEN_USER>()) };
    let sid_len = unsafe { windows_sys::Win32::Security::GetLengthSid(user.User.Sid) };
    if sid_len == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut sid = vec![0u8; sid_len as usize];
    let copied = unsafe {
        windows_sys::Win32::Security::CopySid(sid_len, sid.as_mut_ptr().cast(), user.User.Sid)
    };
    if copied == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(sid)
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
                let received_at_us = boot_timing::timestamp_us();
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let mut response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                    Ok(req) => {
                        log::debug!(
                            "[socket-request] id={} method={} received_at_us={}",
                            req.id.as_ref().unwrap_or(&serde_json::Value::Null),
                            req.method,
                            received_at_us,
                        );
                        respond_to_request(req, received_at_us, mux, pty, notif, output_buf, svc)
                    }
                    Err(e) => {
                        log::debug!("[socket-request] id=null method=invalid received_at_us={received_at_us}");
                        JsonRpcResponse::error(None, -32700, format!("Parse error: {}", e))
                    }
                };

                let after_response = response.after_response.take();
                if let Ok(keep_open) = write_json_rpc_response(reader.get_mut(), &response) {
                    if let Some(after_response) = after_response {
                        after_response();
                    }
                    if !keep_open {
                        break;
                    }
                }
            }
            Err(e) => {
                if is_disconnected_client(&e) {
                    log::debug!("Socket client disconnected before request read: {}", e);
                } else {
                    log::error!("Socket read error: {}", e);
                }
                break;
            }
        }
    }
}

/// Gate the request on the control-plane protocol integer, then dispatch.
fn respond_to_request(
    req: JsonRpcRequest,
    received_at_us: u128,
    mux: &MuxState,
    pty: &PtyState,
    notif: &NotifState,
    output_buf: &OutputBufferState,
    svc: &ServiceState,
) -> JsonRpcResponse {
    if let Some(error) = protocol::version_skew_gate(
        protocol::client_protocol_version(req.protocol_version.as_ref()),
        protocol::PROTOCOL_VERSION,
        protocol::RECOVERY_COMMAND,
    ) {
        return JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: None,
            error: Some(error),
            after_response: None,
        };
    }

    let id = req.id.clone();
    let id_for_panic = id.clone();
    let method = req.method.clone();
    let mut local_params = req.params;
    if let Some(object) = local_params
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
    {
        object.remove("authenticated_mesh_peer_id");
    }
    if let Err(message) = svc.bind_local_rpc(&req.credential, &req.method, &mut local_params) {
        return JsonRpcResponse::error(id, -32003, message);
    }
    if let Err(message) = codeterm_service_core::async_runtime::block_on(
        svc.authorize_neighbor_rpc_credential(&req.credential, &req.method, local_params.as_ref()),
    ) {
        return JsonRpcResponse::error(id, -32003, message);
    }
    match codeterm_service_core::async_runtime::block_on(svc.neighbor_rpc_discovery(
        &req.credential,
        &req.method,
        local_params.as_ref(),
    )) {
        Ok(Some(value)) => return JsonRpcResponse::success(id, value),
        Ok(None) => {}
        Err(message) => return JsonRpcResponse::error(id, -32003, message),
    }
    if req.method == "cli.local.authorize" {
        return match codeterm_service_core::async_runtime::block_on(
            codeterm_service_core::service::cli_local_admission::authorize(
                svc,
                &req.credential,
                local_params.as_ref(),
            ),
        ) {
            Ok(value) => JsonRpcResponse::success(id, value),
            Err(message) => JsonRpcResponse::error(id, -32003, message),
        };
    }
    // catch_unwind so a panic inside any dispatch handler does not silently drop the socket and leave the CLI with an "EOF while parsing" mystery.
    // We surface a real JSON-RPC error instead.
    let caller_tab = match svc.authenticate_local_credential(&req.credential) {
        Ok(caller) => caller.map(|caller| caller.tab_id),
        Err(message) => return JsonRpcResponse::error(id, -32003, message),
    };
    let author = match codeterm_service_core::async_runtime::block_on(
        codeterm_service_core::mem_ops::author::mem_author_for_rpc(
            svc,
            caller_tab.as_deref(),
            &req.method,
        ),
    ) {
        Ok(author) => author,
        Err(message) => return JsonRpcResponse::error(id, -32003, message),
    };
    let dispatched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _arrival = boot_timing::enter_request(id.as_ref(), &req.method, received_at_us);
        let actor = codeterm_service_core::mem_ops::author::author_audit_label(&author);
        codeterm_service_core::mem::store::with_authored_change(author, &actor, || {
            dispatch::dispatch_with_post_response(
                &req.method,
                local_params,
                mux,
                pty,
                notif,
                output_buf,
                svc,
            )
        })
    }));
    match dispatched {
        Ok(Ok((result, after_response))) => {
            let mut response = JsonRpcResponse::success(id, result);
            response.after_response = after_response;
            response
        }
        Ok(Err(err)) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(err),
            after_response: None,
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
mod tests;


#[cfg(test)]
mod process_split_contract;
