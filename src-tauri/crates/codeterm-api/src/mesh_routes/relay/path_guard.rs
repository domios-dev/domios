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
    let bare = codeterm_service_core::api_support::access_policy::request_path(path);
    if bare == "/api/mesh/relay"
        || codeterm_service_core::api_support::access_policy::is_local_only_path(bare)
    {
        return Err(format!("Relay target is local-only or nested: {path}"));
    }
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

pub(super) fn is_runtime_login_code_relay(
    method: &str,
    path: &str,
    body: &serde_json::Value,
) -> bool {
    let bare = path.split('?').next().unwrap_or(path);
    method.eq_ignore_ascii_case("POST")
        && bare.starts_with("/api/tabs/")
        && bare.ends_with("/agent/runtime/login")
        && (body.get("action").and_then(serde_json::Value::as_str) == Some("code")
            || body.get("code").is_some_and(|value| !value.is_null()))
}

pub(super) fn refuse_non_owner_login_code(
    allowed: bool,
    is_login_code: bool,
    body: &mut serde_json::Value,
) -> Result<(), (axum::http::StatusCode, String)> {
    if is_login_code && !allowed {
        zeroize_login_body(body);
        return Err((
            axum::http::StatusCode::FORBIDDEN,
            "Only the local owner may submit a remote sign-in code".into(),
        ));
    }
    Ok(())
}

pub(super) fn is_sensitive_relay(method: &str, path: &str, body: &serde_json::Value) -> bool {
    let bare = path.split('?').next().unwrap_or(path);
    is_runtime_login_code_relay(method, path, body)
        || (method.eq_ignore_ascii_case("POST") && bare == "/api/secure-input/deliver")
}

pub(super) fn zeroize_login_body(body: &mut serde_json::Value) {
    codeterm_service_core::service::secure_input::zeroize_secure_input_json_value(body);
}

fn is_dot_segment(segment: &str) -> bool {
    matches!(
        segment.to_ascii_lowercase().replace("%2e", ".").as_str(),
        "." | ".."
    )
}
