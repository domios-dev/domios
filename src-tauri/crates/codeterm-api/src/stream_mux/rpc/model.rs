use axum::http::{Method, Uri};

pub(crate) const IN_FLIGHT_CAP: usize = 64;
pub(crate) const BULK_CAP: usize = 48;
pub(crate) const STREAM_CAP: usize = 256;

#[derive(serde::Deserialize)]
pub(crate) struct RelayOperation {
    pub method: String,
    pub path: String,
}

pub(crate) fn validate_path(path: &str) -> Result<Uri, String> {
    if !path.starts_with("/api/") || path.contains(['\\', '#']) {
        return Err("Mux path must be origin-relative API path".into());
    }
    let uri: Uri = path.parse().map_err(|_| "Invalid mux path")?;
    let mut decoded = Vec::new();
    let mut bytes = uri.path().bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let hi = bytes.next().and_then(|b| (b as char).to_digit(16));
            let lo = bytes.next().and_then(|b| (b as char).to_digit(16));
            match (hi, lo) {
                (Some(hi), Some(lo)) => decoded.push((hi * 16 + lo) as u8),
                _ => return Err("Invalid path escape".into()),
            }
        } else {
            decoded.push(byte);
        }
    }
    let decoded = String::from_utf8(decoded).map_err(|_| "Invalid UTF-8 path")?;
    if decoded.starts_with("/api/stream-mux")
        || decoded.contains('\\')
        || decoded.chars().any(char::is_control)
        || decoded.split('/').any(|part| part == "." || part == "..")
    {
        return Err("Recursive or invalid mux path".into());
    }
    Ok(uri)
}

fn read_post(path: &str) -> bool {
    if path == "/api/essence-maps" {
        return true;
    }
    let Some(rest) = path.strip_prefix("/api/tabs/") else {
        return false;
    };
    let Some((tab, operation)) = rest.split_once('/') else {
        return false;
    };
    !tab.is_empty()
        && matches!(
            operation,
            "preview/read"
                | "preview/check"
                | "chat-raw/start"
                | "chat-raw/history"
                | "chat-raw/backfill"
        )
}

pub(crate) fn cancellable(method: &Method, path: &str, relay: Option<&RelayOperation>) -> bool {
    if method == Method::GET || method == Method::HEAD {
        return true;
    }
    if method != Method::POST {
        return false;
    }
    if path == "/api/mesh/relay-to" {
        return relay.is_some_and(|op| {
            let Ok(uri) = validate_path(&op.path) else {
                return false;
            };
            matches!(op.method.as_str(), "GET" | "HEAD")
                || op.method == "POST" && read_post(uri.path())
        });
    }
    read_post(path)
}

pub(crate) fn admission(
    owner_matches: bool,
    duplicate: bool,
    stream: bool,
    streams: usize,
    requests: usize,
    bulk: usize,
    is_bulk: bool,
) -> Result<(), u16> {
    if !owner_matches {
        return Err(403);
    }
    if duplicate {
        return Err(409);
    }
    if if stream {
        streams >= STREAM_CAP
    } else {
        requests >= IN_FLIGHT_CAP || is_bulk && bulk >= BULK_CAP
    } {
        return Err(429);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
