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

/// Explain why authenticated secret HTTP can fail while the daemon socket is live.
pub(crate) fn mem_unavailable_message(base: &str, live_socket: Option<&str>) -> String {
    match live_socket {
        Some(socket) => format!(
            "CodeTerm secret management is unavailable: the daemon socket '{socket}' is live, but its HTTP API at {base} refused the connection. \
             `mem secret` is authenticated-HTTP-only by design (it can return plaintext credentials, so it deliberately stays off the unauthenticated local socket) — a running daemon socket is not enough. \
             Check the daemon's HTTP listener in its logs, or set CODETERM_API_PORT to the port this instance actually serves."
        ),
        None => format!(
            "CodeTerm secret management is unavailable: no CodeTerm daemon is reachable — the HTTP API at {base} refused the connection and no daemon socket is live. \
             Start CodeTerm, or set CODETERM_API_PORT / CODETERM_SOCKET_PATH to the instance you meant to reach."
        ),
    }
}
