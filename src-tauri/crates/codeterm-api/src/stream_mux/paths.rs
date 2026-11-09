use axum::http::Uri;

pub(crate) fn classify_channel_path(path: &str) -> Result<Uri, String> {
    if !path.starts_with('/') || path.starts_with("//") || path.contains('\\') {
        return Err("Mux channel path must be origin-relative".into());
    }
    let uri: Uri = path
        .parse()
        .map_err(|_| format!("Invalid mux channel path: {path}"))?;
    let segments: Vec<_> = uri.path().split('/').collect();
    let allowed = matches!(
        uri.path(),
        "/api/mesh/relay-stream"
            | "/api/output/stream"
            | "/api/events/stream"
            | "/api/startup/events"
    ) || matches!(segments.as_slice(), ["", "api", "tabs", id, "output", "stream"] if !id.is_empty())
        || matches!(segments.as_slice(), ["", "api", "surfaces", id, "stream"] if !id.is_empty());
    if allowed {
        Ok(uri)
    } else {
        Err(format!("Not a multiplexable stream path: {}", uri.path()))
    }
}
