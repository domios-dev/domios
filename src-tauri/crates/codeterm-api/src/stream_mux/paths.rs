use axum::http::Uri;

pub(crate) fn replayable_output(uri: &Uri) -> bool {
    fn output_path(path: &str) -> bool {
        path == "/api/output/stream"
            || matches!(path.split('/').collect::<Vec<_>>().as_slice(),
            ["", "api", "tabs", id, "output", "stream"] if !id.is_empty())
    }
    if output_path(uri.path()) {
        return true;
    }
    uri.path() == "/api/mesh/relay-stream"
        && url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes())
            .find(|(key, _)| key == "path")
            .and_then(|(_, path)| path.parse::<Uri>().ok())
            .is_some_and(|inner| {
                inner.scheme().is_none() && inner.authority().is_none() && output_path(inner.path())
            })
}

#[cfg(test)]
mod tests;

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
