use super::*;
use std::error::Error;
use std::io::ErrorKind;
use std::time::Duration;

fn absent(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::NotFound | ErrorKind::ConnectionRefused)
}

fn scan_production(api_port: Option<&str>, api_url: Option<&str>) -> bool {
    api_url.is_none_or(|url| url.trim().is_empty())
        && api_port.is_none_or(|port| port.trim().is_empty() || port.trim() == "7685")
}

fn connection_absent(error: &reqwest::Error) -> bool {
    let mut source: Option<&(dyn Error + 'static)> = Some(error);
    while let Some(error) = source {
        if let Some(error) = error.downcast_ref::<std::io::Error>() {
            return absent(error.kind());
        }
        source = error.source();
    }
    false
}

impl SocketClient {
    pub fn connect_optional_local() -> Result<Option<Self>, String> {
        let selected = resolve_socket_selection(
            std::env::var("CODETERM_SOCKET_PATH").ok().as_deref(),
            std::env::var("CODETERM_INSTANCE").ok().as_deref(),
            std::env::var("CODETERM_API_PORT").ok().as_deref(),
        )?;
        let mut candidates = match selected {
            Some(selected) => vec![selected.path],
            None if !scan_production(
                std::env::var("CODETERM_API_PORT").ok().as_deref(),
                std::env::var("CODETERM_API_URL").ok().as_deref(),
            ) =>
            {
                Vec::new()
            }
            None => {
                let root = if cfg!(windows) { r"\\.\pipe" } else { "/tmp" };
                let mut names = std::fs::read_dir(root)
                    .map_err(|error| error.to_string())?
                    .map(|entry| {
                        entry.map(|entry| entry.file_name().to_string_lossy().into_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                names.retain(|name| is_daemon_socket_name(name) && !is_dev_socket(name));
                names.sort();
                let mut ordered = Vec::new();
                while let Some(name) = pick_preferred_socket(&names, prefers_dev_socket()) {
                    names.retain(|candidate| candidate != &name);
                    ordered.push(name);
                }
                ordered
            }
        };
        for candidate in candidates.clone() {
            if let Some(bare) = candidate.strip_suffix(".sock") {
                candidates.push(bare.into());
            } else {
                candidates.push(format!("{candidate}.sock"));
            }
        }
        let mut socket_error = None;
        for candidate in candidates {
            match Self::try_connect(&candidate) {
                Ok(client) => return Ok(Some(client)),
                Err(error) if absent(error.kind()) => {}
                Err(error) => socket_error = Some(error.to_string()),
            }
        }
        let url = startup::target_url()?;
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|error| error.to_string())?;
        match client.get(format!("{url}/api/health")).send() {
            Ok(_) => Self::connect_http(http_fallback_config()?).map(Some),
            Err(error) if connection_absent(&error) && socket_error.is_none() => Ok(None),
            Err(error) => Err(format!(
                "Cannot establish whether Domios is offline: {error}; socket: {socket_error:?}"
            )),
        }
    }
}

#[cfg(test)]
mod tests;
