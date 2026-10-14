/// Rejects relay paths that could leave `/api/` on loopback (userinfo, schemes,
/// protocol-relative) or re-resolve to another tab (dot segments, backslashes).
pub(super) fn validate_relay_path(path: &str) -> Result<(), String> {
    if !path.starts_with("/api/") {
        return Err(format!("Relay path must start with /api/: {path}"));
    }
    if path.contains('@') {
        return Err(format!(
            "Relay path must not contain '@' (SSRF via userinfo): {path}"
        ));
    }
    if path.starts_with("//") {
        return Err(format!(
            "Relay path must not start with // (protocol-relative): {path}"
        ));
    }
    let bare = crate::api::access_policy::request_path(path);
    if bare.contains('\\') || bare.split('/').any(is_dot_segment) {
        return Err(format!(
            "Relay path must not contain dot segments or backslashes: {path}"
        ));
    }
    // Block explicit scheme prefixes embedded in the path (e.g. /api/http://evil).
    for scheme in &["http://", "https://", "file://", "ftp://", "data:"] {
        if path.contains(scheme) {
            return Err(format!(
                "Relay path must not contain scheme '{scheme}': {path}"
            ));
        }
    }
    Ok(())
}

fn is_dot_segment(segment: &str) -> bool {
    matches!(
        segment.to_ascii_lowercase().replace("%2e", ".").as_str(),
        "." | ".."
    )
}
