pub(super) const CHANNEL_UNAVAILABLE_PREFIX: &str = "Plugin channel unavailable: ";

pub(super) const REPOSITORY_NOT_FOUND: &str = "repository not found or not readable without access";
pub(super) const AUTHENTICATION_REQUIRED: &str = "authentication required";
pub(super) const NETWORK_UNAVAILABLE: &str = "network unavailable";
pub(super) const ASSET_NOT_FOUND: &str = "release asset not found";
pub(super) const ACCESS_DENIED: &str = "access denied";
pub(super) const INTEGRITY_FAILED: &str = "download failed its integrity check";

pub(super) fn channel_unavailable(cause: &str, url: &str) -> String {
    format!(
        "{CHANNEL_UNAVAILABLE_PREFIX}{cause} ({}). Domios will retry.",
        source_label(url)
    )
}

pub(super) fn git_failure_cause(stderr: &str) -> String {
    let text = stderr.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if has(&[
        "repository not found",
        "does not appear to be a git repository",
    ]) || (text.contains("repository '") && text.contains("' not found"))
    {
        return REPOSITORY_NOT_FOUND.into();
    }
    if has(&[
        "could not read username",
        "could not read password",
        "authentication failed",
        "invalid username or password",
        "permission denied (publickey",
        "returned error: 401",
        "returned error: 403",
    ]) {
        return AUTHENTICATION_REQUIRED.into();
    }
    if has(&[
        "could not resolve host",
        "failed to connect",
        "connection timed out",
        "connection refused",
        "network is unreachable",
        "operation timed out",
        "couldn't connect",
        "ssl certificate",
        "ssl connect",
        "tls handshake",
        "gnutls",
        "schannel",
    ]) {
        return NETWORK_UNAVAILABLE.into();
    }
    match diagnostic_block(stderr) {
        Some(line) => format!("git failed: {line}"),
        None => "git failed".into(),
    }
}

pub(super) fn http_failure_cause(error: &reqwest::Error) -> String {
    match error.status().map(|status| status.as_u16()) {
        Some(404) => ASSET_NOT_FOUND.into(),
        Some(401 | 403) => ACCESS_DENIED.into(),
        Some(status) => format!("server returned HTTP {status}"),
        None if error.is_connect() || error.is_timeout() || error.is_request() => {
            NETWORK_UNAVAILABLE.into()
        }
        None if error.is_redirect() => "unsafe redirect refused".into(),
        None => "download failed".into(),
    }
}

/// Host and path only: credentials, query and fragment never reach a user-visible message.
pub(super) fn source_label(url: &str) -> String {
    let base = url.split('#').next().unwrap_or(url);
    if let Ok(parsed) = reqwest::Url::parse(base) {
        if let Some(host) = parsed.host_str() {
            return format!("{host}{}", parsed.path());
        }
    }
    let without_user = base.rsplit_once('@').map_or(base, |(_, rest)| rest);
    without_user
        .split('?')
        .next()
        .unwrap_or(without_user)
        .into()
}

fn diagnostic_block(stderr: &str) -> Option<String> {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let first = lines
        .iter()
        .position(|line| {
            let lower = line.to_ascii_lowercase();
            lower.starts_with("fatal:")
                || lower.starts_with("error:")
                || lower.starts_with("remote: error:")
        })
        .unwrap_or(0);
    let detail = lines.get(first..)?.join("\n");
    if detail.is_empty() {
        None
    } else {
        Some(detail.chars().take(4096).collect())
    }
}

#[cfg(test)]
mod tests;
