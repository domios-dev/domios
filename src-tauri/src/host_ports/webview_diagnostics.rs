use codeterm_types::native_host::{
    NativeHostConnection, NativeWebviewDiagnosticsSnapshot, NativeWebviewDiagnosticsStatus,
    NativeWebviewDiagnosticsUnavailableReason,
};
use codeterm_types::server_bootstrap::ProcessProvenance;
use parking_lot::Mutex;
use reqwest::redirect::Policy;
#[cfg(windows)]
use std::net::{Ipv4Addr, SocketAddrV4};
use std::{io, time::Duration};
use url::Url;
use uuid::Uuid;

const WEBVIEW_DIAGNOSTICS_DEADLINE: Duration = Duration::from_secs(2);
const MAX_WEBVIEW_DIAGNOSTICS_BYTES: usize = 64 * 1024;
pub(crate) const CLIENT_VIEW_QUERY: &str = "__codeterm_client_view";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClientViewGeneration {
    pub(crate) view: u64,
    pub(crate) navigation: u64,
}

#[cfg(any(debug_assertions, test))]
pub(crate) fn dev_locator_enabled(
    debug: bool,
    selector: &codeterm_types::server_bootstrap::InstanceSelector,
) -> bool {
    debug
        && matches!(
            selector,
            codeterm_types::server_bootstrap::InstanceSelector::Development { .. }
        )
}

pub(crate) struct DiagnosticsSession {
    requested_port: Option<u16>,
    preflight: Mutex<PortPreflight>,
    status: Mutex<Option<NativeWebviewDiagnosticsStatus>>,
}

impl DiagnosticsSession {
    pub(crate) fn new(mode: crate::LocalWebviewDiagnosticsMode) -> Self {
        let requested_port = match mode {
            crate::LocalWebviewDiagnosticsMode::Disabled => None,
            crate::LocalWebviewDiagnosticsMode::RequestedPort(port) => Some(port),
        };
        Self {
            requested_port,
            preflight: Mutex::new(PortPreflight::pending(
                cfg!(debug_assertions),
                cfg!(windows),
            )),
            status: Mutex::new(None),
        }
    }

    pub(crate) fn set_port_preflight(&self, port: u16, preflight: PortPreflight) {
        if self.requested_port == Some(port) {
            *self.preflight.lock() = preflight;
        }
    }

    #[cfg(debug_assertions)]
    pub(crate) fn preflight_receipt(&self) -> serde_json::Value {
        self.preflight.lock().receipt(self.requested_port)
    }

    pub(crate) fn bind_view(&self) -> NativeWebviewDiagnosticsStatus {
        let status = initial_status(self.requested_port, *self.preflight.lock());
        *self.status.lock() = Some(status.clone());
        status
    }

    pub(crate) fn release_view(&self) {
        self.status.lock().take();
    }

    pub(crate) async fn inspect_view(
        &self,
        generation: ClientViewGeneration,
        url: Url,
    ) -> NativeWebviewDiagnosticsStatus {
        let preflight = *self.preflight.lock();
        status_for_view(self.requested_port, generation, url, preflight).await
    }

    pub(crate) fn adopt_status(&self, status: NativeWebviewDiagnosticsStatus) {
        *self.status.lock() = Some(status);
    }

    pub(crate) fn snapshot(
        &self,
        connection: &NativeHostConnection,
        view_session: Uuid,
        client_process: &ProcessProvenance,
    ) -> Option<NativeWebviewDiagnosticsSnapshot> {
        Some(NativeWebviewDiagnosticsSnapshot {
            connection: connection.clone(),
            view_session,
            client_process: client_process.clone(),
            status: self.status.lock().clone()?,
        })
    }
}

#[cfg(any(windows, test))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct ProcessIdentity {
    pid: u32,
    parent_pid: u32,
    image: String,
}

#[cfg(any(windows, test))]
fn is_owned_webview_listener(
    listener_pid: u32,
    client_pid: u32,
    processes: &[ProcessIdentity],
) -> bool {
    let Some(listener) = processes.iter().find(|process| process.pid == listener_pid) else {
        return false;
    };
    if !listener.image.eq_ignore_ascii_case("msedgewebview2.exe") {
        return false;
    }
    let mut current = listener_pid;
    for _ in 0..64 {
        if current == client_pid {
            return true;
        }
        let Some(process) = processes.iter().find(|process| process.pid == current) else {
            return false;
        };
        if process.parent_pid == current {
            return false;
        }
        current = process.parent_pid;
    }
    false
}

pub(crate) fn root_generation_from_url(url: &Url) -> Option<ClientViewGeneration> {
    let value = url
        .query_pairs()
        .filter(|(name, _)| name == CLIENT_VIEW_QUERY)
        .map(|(_, value)| value.into_owned())
        .last()?;
    let (view, navigation) = value.split_once(':')?;
    Some(ClientViewGeneration {
        view: view.parse().ok()?,
        navigation: navigation.parse().ok()?,
    })
}

pub(crate) fn with_root_generation(url: &Url, generation: ClientViewGeneration) -> Url {
    let retained = url
        .query_pairs()
        .filter(|(name, _)| name != CLIENT_VIEW_QUERY)
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let mut url = url.clone();
    url.set_query(None);
    {
        let mut query = url.query_pairs_mut();
        for (name, value) in retained {
            query.append_pair(&name, &value);
        }
        query.append_pair(
            CLIENT_VIEW_QUERY,
            &format!("{}:{}", generation.view, generation.navigation),
        );
    }
    url
}

fn without_root_generation(url: &Url) -> Url {
    let retained = url
        .query_pairs()
        .filter(|(name, _)| name != CLIENT_VIEW_QUERY)
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let mut url = url.clone();
    url.set_query(None);
    if !retained.is_empty() {
        let mut query = url.query_pairs_mut();
        for (name, value) in retained {
            query.append_pair(&name, &value);
        }
    }
    url.set_fragment(None);
    url
}

fn target_matches(
    target_type: &str,
    target_url: &str,
    web_socket_debugger_url: &str,
    port: u16,
    generation: ClientViewGeneration,
    main_url: &Url,
) -> bool {
    if target_type != "page" {
        return false;
    }
    let Ok(target_url) = Url::parse(target_url) else {
        return false;
    };
    let Ok(web_socket) = Url::parse(web_socket_debugger_url) else {
        return false;
    };
    root_generation_from_url(&target_url) == Some(generation)
        && without_root_generation(&target_url) == without_root_generation(main_url)
        && web_socket.scheme() == "ws"
        && web_socket
            .host_str()
            .is_some_and(|host| host == "127.0.0.1" || host == "localhost" || host == "::1")
        && web_socket.port() == Some(port)
        && web_socket.path().starts_with("/devtools/page/")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindOutcome {
    NotAttempted,
    Available,
    Failed {
        kind: io::ErrorKind,
        raw_os_error: Option<i32>,
    },
}

#[cfg(debug_assertions)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
struct FailureListenerObservation {
    listener_pid_before: Option<u32>,
    listener_pid_after: Option<u32>,
    client_ancestry_verified: Option<bool>,
}

#[cfg(debug_assertions)]
impl FailureListenerObservation {
    fn capture(
        mut listener_pid: impl FnMut() -> Option<u32>,
        ancestry_matches: impl FnOnce(u32) -> bool,
    ) -> Self {
        let before = listener_pid();
        let ancestry = before.map(ancestry_matches);
        let after = listener_pid();
        Self {
            listener_pid_before: before,
            listener_pid_after: after,
            client_ancestry_verified: if before == after { ancestry } else { None },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PortPreflight {
    debug: bool,
    windows: bool,
    bind: BindOutcome,
    #[cfg(debug_assertions)]
    failure_listener: Option<FailureListenerObservation>,
    #[cfg(debug_assertions)]
    starting_view_present: Option<bool>,
}

impl PortPreflight {
    fn pending(debug: bool, windows: bool) -> Self {
        Self {
            debug,
            windows,
            bind: BindOutcome::NotAttempted,
            #[cfg(debug_assertions)]
            failure_listener: None,
            #[cfg(debug_assertions)]
            starting_view_present: None,
        }
    }

    pub(crate) fn evaluate(
        debug: bool,
        windows: bool,
        bind: impl FnOnce() -> io::Result<()>,
    ) -> Self {
        let mut preflight = Self::pending(debug, windows);
        if debug && windows {
            preflight.bind = match bind() {
                Ok(()) => BindOutcome::Available,
                Err(error) => BindOutcome::Failed {
                    kind: error.kind(),
                    raw_os_error: error.raw_os_error(),
                },
            };
        }
        preflight
    }

    pub(crate) fn available(self) -> bool {
        self.debug && self.windows && self.bind == BindOutcome::Available
    }

    #[cfg(debug_assertions)]
    fn observe_address_in_use(
        mut self,
        observe: impl FnOnce() -> FailureListenerObservation,
    ) -> Self {
        if self.debug
            && self.windows
            && matches!(self.bind, BindOutcome::Failed {
                kind: io::ErrorKind::AddrInUse, ..
            })
        {
            self.failure_listener = Some(observe());
        }
        self
    }

    #[cfg(debug_assertions)]
    pub(crate) fn with_starting_view_present(mut self, present: bool) -> Self {
        self.starting_view_present = Some(present);
        self
    }

    #[cfg(debug_assertions)]
    fn receipt(self, requested_port: Option<u16>) -> serde_json::Value {
        let (outcome, kind, raw_os_error) = match self.bind {
            BindOutcome::NotAttempted => ("not_attempted", None, None),
            BindOutcome::Available => ("available", None, None),
            BindOutcome::Failed { kind, raw_os_error } => (
                if kind == io::ErrorKind::AddrInUse {
                    "address_in_use"
                } else {
                    "bind_failed"
                },
                Some(format!("{kind:?}")),
                raw_os_error,
            ),
        };
        serde_json::json!({
            "requested_port": requested_port,
            "compiled_debug": self.debug,
            "compiled_windows": self.windows,
            "phase": "after_builder_before_build",
            "bind_outcome": outcome,
            "bind_error_kind": kind,
            "raw_os_error": raw_os_error,
            "failure_listener": self.failure_listener,
            "starting_view_present": self.starting_view_present,
        })
    }
}

pub(crate) fn port_preflight(port: u16) -> PortPreflight {
    let preflight = PortPreflight::evaluate(cfg!(debug_assertions), cfg!(windows), || {
        #[cfg(windows)]
        {
            codeterm_util::tcp_listener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into())
                .map(drop)
        }
        #[cfg(not(windows))]
        {
            Ok(())
        }
    });
    #[cfg(debug_assertions)]
    let preflight = preflight.observe_address_in_use(|| {
        #[cfg(windows)]
        {
            FailureListenerObservation::capture(
                || windows_listener::listener_process_id(port),
                |pid| windows_listener::listener_is_owned_by_client(pid, std::process::id()),
            )
        }
        #[cfg(not(windows))]
        {
            FailureListenerObservation::capture(|| None, |_| false)
        }
    });
    log::debug!(
        "[webview-diagnostics] preflight requested_port={port} phase=after_builder_before_build {preflight:?}"
    );
    preflight
}

pub(crate) fn initial_status(
    requested_port: Option<u16>,
    preflight: PortPreflight,
) -> NativeWebviewDiagnosticsStatus {
    match requested_port {
        None => NativeWebviewDiagnosticsStatus::Disabled,
        Some(_) if !preflight.debug || !preflight.windows => {
            NativeWebviewDiagnosticsStatus::Unsupported
        }
        Some(_)
            if matches!(
                preflight.bind,
                BindOutcome::Failed {
                    kind: io::ErrorKind::AddrInUse,
                    ..
                }
            ) =>
        {
            NativeWebviewDiagnosticsStatus::Unavailable {
                reason: NativeWebviewDiagnosticsUnavailableReason::PortOccupied,
            }
        }
        Some(_) if matches!(preflight.bind, BindOutcome::Failed { .. }) => {
            NativeWebviewDiagnosticsStatus::Unavailable {
                reason: NativeWebviewDiagnosticsUnavailableReason::WebviewInitializationFailed,
            }
        }
        Some(_) => NativeWebviewDiagnosticsStatus::Unavailable {
            reason: NativeWebviewDiagnosticsUnavailableReason::EndpointNotAttributed,
        },
    }
}

pub(crate) async fn status_for_view(
    requested_port: Option<u16>,
    generation: ClientViewGeneration,
    main_url: Url,
    preflight: PortPreflight,
) -> NativeWebviewDiagnosticsStatus {
    match requested_port {
        None => NativeWebviewDiagnosticsStatus::Disabled,
        Some(_) if !preflight.debug || !preflight.windows => {
            NativeWebviewDiagnosticsStatus::Unsupported
        }
        Some(port) => inspect_main_webview_diagnostics(port, generation, main_url, preflight).await,
    }
}

pub(crate) async fn inspect_main_webview_diagnostics(
    port: u16,
    generation: ClientViewGeneration,
    main_url: Url,
    preflight: PortPreflight,
) -> NativeWebviewDiagnosticsStatus {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Target {
        #[serde(rename = "type")]
        target_type: String,
        url: String,
        web_socket_debugger_url: Option<String>,
    }

    if !preflight.available() {
        return initial_status(Some(port), preflight);
    }
    let unavailable = || NativeWebviewDiagnosticsStatus::Unavailable {
        reason: NativeWebviewDiagnosticsUnavailableReason::EndpointNotAttributed,
    };
    if !cfg!(windows)
        || port < 1024
        || generation.view == 0
        || generation.navigation == 0
        || root_generation_from_url(&main_url) != Some(generation)
    {
        return unavailable();
    }
    #[cfg(windows)]
    let Some(listener_pid) = windows_listener::listener_process_id(port) else {
        return unavailable();
    };
    #[cfg(windows)]
    if !windows_listener::listener_is_owned_by_client(listener_pid, std::process::id()) {
        return unavailable();
    }
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(WEBVIEW_DIAGNOSTICS_DEADLINE)
        .build()
    else {
        return unavailable();
    };
    let Ok(mut response) = client
        .get(format!("http://127.0.0.1:{port}/json/list"))
        .send()
        .await
    else {
        return unavailable();
    };
    if !response.status().is_success() {
        return unavailable();
    }
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk))
                if bytes.len().saturating_add(chunk.len()) <= MAX_WEBVIEW_DIAGNOSTICS_BYTES =>
            {
                bytes.extend_from_slice(&chunk);
            }
            Ok(Some(_)) | Err(_) => return unavailable(),
            Ok(None) => break,
        }
    }
    let Ok(targets) = serde_json::from_slice::<Vec<Target>>(&bytes) else {
        return unavailable();
    };
    #[cfg(windows)]
    if windows_listener::listener_process_id(port) != Some(listener_pid)
        || !windows_listener::listener_is_owned_by_client(listener_pid, std::process::id())
    {
        return unavailable();
    }
    let attributed = targets.into_iter().any(|target| {
        let Some(web_socket) = target.web_socket_debugger_url.as_deref() else {
            return false;
        };
        target_matches(
            &target.target_type,
            &target.url,
            web_socket,
            port,
            generation,
            &main_url,
        )
    });
    if attributed {
        NativeWebviewDiagnosticsStatus::Active {
            port,
            view_generation: generation.view,
        }
    } else {
        unavailable()
    }
}

#[cfg(windows)]
mod windows_listener {
    use super::{is_owned_webview_listener, ProcessIdentity};
    use std::{
        ffi::c_void,
        mem::{size_of, zeroed},
        ptr,
    };

    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    const AF_INET: u32 = 2;
    const TCP_TABLE_OWNER_PID_LISTENER: i32 = 3;
    const TH32CS_SNAPPROCESS: u32 = 2;
    #[repr(C)]
    struct TcpRowOwnerPid {
        _state: u32,
        local_address: u32,
        local_port: u32,
        _remote_address: u32,
        _remote_port: u32,
        process_id: u32,
    }

    #[repr(C)]
    struct ProcessEntry32W {
        size: u32,
        usage: u32,
        process_id: u32,
        default_heap_id: usize,
        module_id: u32,
        thread_count: u32,
        parent_process_id: u32,
        base_priority: i32,
        flags: u32,
        image: [u16; 260],
    }

    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedTcpTable(
            table: *mut c_void,
            size: *mut u32,
            order: i32,
            family: u32,
            table_class: i32,
            reserved: u32,
        ) -> u32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    pub(super) fn listener_process_id(port: u16) -> Option<u32> {
        let mut required = 0u32;
        let result = unsafe {
            GetExtendedTcpTable(
                ptr::null_mut(),
                &mut required,
                0,
                AF_INET,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if result != ERROR_INSUFFICIENT_BUFFER || required < size_of::<u32>() as u32 {
            return None;
        }
        if required as usize > 1024 * 1024 {
            return None;
        }
        let words = (required as usize).checked_add(size_of::<u64>() - 1)? / size_of::<u64>();
        let mut storage = vec![0u64; words];
        let result = unsafe {
            GetExtendedTcpTable(
                storage.as_mut_ptr().cast(),
                &mut required,
                0,
                AF_INET,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if result != 0 {
            return None;
        }
        let count = unsafe { ptr::read(storage.as_ptr().cast::<u32>()) as usize };
        let table_bytes =
            size_of::<u32>().checked_add(count.checked_mul(size_of::<TcpRowOwnerPid>())?)?;
        if table_bytes > required as usize || table_bytes > storage.len() * size_of::<u64>() {
            return None;
        }
        let rows = unsafe {
            std::slice::from_raw_parts(
                storage
                    .as_ptr()
                    .cast::<u8>()
                    .add(size_of::<u32>())
                    .cast::<TcpRowOwnerPid>(),
                count,
            )
        };
        rows.iter()
            .find(|row| {
                row.local_address.to_ne_bytes() == [127, 0, 0, 1]
                    && u16::from_be(row.local_port as u16) == port
            })
            .map(|row| row.process_id)
    }

    pub(super) fn listener_is_owned_by_client(listener_pid: u32, client_pid: u32) -> bool {
        let Some(processes) = process_snapshot() else {
            return false;
        };
        is_owned_webview_listener(listener_pid, client_pid, &processes)
    }

    fn process_snapshot() -> Option<Vec<ProcessIdentity>> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == -1isize as *mut c_void {
            return None;
        }
        let mut records = Vec::new();
        let mut entry: ProcessEntry32W = unsafe { zeroed() };
        entry.size = size_of::<ProcessEntry32W>() as u32;
        let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while has_entry {
            let end = entry
                .image
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(entry.image.len());
            records.push(ProcessIdentity {
                pid: entry.process_id,
                parent_pid: entry.parent_process_id,
                image: String::from_utf16_lossy(&entry.image[..end]),
            });
            has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        unsafe {
            CloseHandle(snapshot);
        }
        Some(records)
    }
}

#[cfg(test)]
mod tests;
