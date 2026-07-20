use crate::client::live_socket_name;

/// Map a `mem` HTTP failure to a user-facing error.
///
/// A refused connection is the one failure users cannot act on from reqwest's
/// own text, so it gets an explicit diagnosis; everything else passes through.
pub(crate) fn mem_transport_error(url: &str, err: &reqwest::Error) -> String {
    if err.is_connect() {
        return mem_unavailable_message(&api_origin(url), live_socket_name().as_deref());
    }
    err.to_string()
}

fn api_origin(url: &str) -> String {
    reqwest::Url::parse(url)
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_else(|_| url.to_string())
}

/// This subcommand group (skill/graph/worker/calibration/secret management)
/// still speaks only HTTP, so a live daemon socket says nothing about
/// whether it can work — spell that out instead of leaking "connection refused".
/// The core `mem` item surface (get/save/update/delete/search/...) no longer
/// hits this path at all: it's socket-routed and can't reach a dead HTTP port.
pub(crate) fn mem_unavailable_message(base: &str, live_socket: Option<&str>) -> String {
    match live_socket {
        Some(socket) => format!(
            "CodeTerm memory is unavailable: the daemon socket '{socket}' is live, but its HTTP API at {base} refused the connection. \
             This mem subcommand (skill/graph/worker/calibration/secret management) is HTTP-only and is not exposed over the socket, so a running daemon is not enough. \
             Check the daemon's HTTP listener in its logs, or set CODETERM_API_PORT to the port this instance actually serves."
        ),
        None => format!(
            "CodeTerm memory is unavailable: no CodeTerm daemon is reachable — the HTTP API at {base} refused the connection and no daemon socket is live. \
             Start CodeTerm, or set CODETERM_API_PORT / CODETERM_SOCKET_PATH to the instance you meant to reach."
        ),
    }
}
