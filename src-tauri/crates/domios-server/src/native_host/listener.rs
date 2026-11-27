use codeterm_types::server_bootstrap::ServerReady;
use interprocess::local_socket::{
    tokio::{prelude::*, Listener},
    GenericNamespaced, ListenerOptions,
};
use std::io;
use tokio::sync::{mpsc, oneshot};

pub(super) fn create_listener(ready: &ServerReady) -> io::Result<Listener> {
    let address = codeterm_types::native_host::native_host_socket_address(&ready.bootstrap_address)
        .ok_or_else(|| io::Error::other("Invalid native host bootstrap address"))?;
    codeterm_util::local_endpoint::prepare_namespaced(&address)?;
    let target = codeterm_util::local_endpoint::display_name(&address);
    let name = address
        .to_ns_name::<GenericNamespaced>()
        .map_err(|error| codeterm_util::local_endpoint::bind_error(&target, error))?;
    let options = ListenerOptions::new().name(name);
    // fchmod on a socket is EINVAL on macOS (interprocess: Unsupported); only Linux pins the mode.
    #[cfg(target_os = "linux")]
    let options = {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        options.mode(0o600)
    };
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::{
            local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
        };
        let sddl = widestring::U16CString::from_str("D:P(A;;GA;;;OW)").map_err(io::Error::other)?;
        options.security_descriptor(SecurityDescriptor::deserialize(&sddl)?)
    };
    options
        .create_tokio()
        .map_err(|error| codeterm_util::local_endpoint::bind_error(&target, error))
}

pub(super) fn is_transient_accept_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock
            | io::ErrorKind::Interrupted
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::TimedOut
    )
}

pub(super) fn can_replace_active_session(
    retired: bool,
    control_closed: bool,
    owner_matches: bool,
    same_process: bool,
) -> bool {
    retired || control_closed || !owner_matches || same_process
}

pub(super) async fn recv_rebind_check(
    receiver: &mut Option<mpsc::Receiver<oneshot::Sender<()>>>,
) -> oneshot::Sender<()> {
    if let Some(rx) = receiver.as_mut() {
        if let Some(ack) = rx.recv().await {
            return ack;
        }
        *receiver = None;
    }
    std::future::pending().await
}

pub(super) fn native_socket_path_missing(ready: &ServerReady) -> bool {
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
    {
        let Some(address) =
            codeterm_types::native_host::native_host_socket_address(&ready.bootstrap_address)
        else {
            return false;
        };
        let path = codeterm_util::local_endpoint::display_name(&address);
        std::fs::symlink_metadata(&path).is_err()
    }
    #[cfg(any(windows, target_os = "linux", target_os = "android"))]
    {
        let _ = ready;
        false
    }
}
