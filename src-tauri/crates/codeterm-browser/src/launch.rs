use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::locator::HostOs;

pub const OWNERSHIP_FLAG: &str = "--codeterm-owned-browser";

const DEVTOOLS_MARKER: &str = "DevTools listening on ";
const MAX_PENDING_LINE: usize = 64 * 1024;
const TAIL_LINES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacOsAppLaunch {
    pub bundle: PathBuf,
    pub profile_dir: PathBuf,
    pub adds_to_recent_items: bool,
    pub creates_new_instance: bool,
    pub activates: bool,
}

pub fn macos_app_launch(
    os: HostOs,
    binary: &Path,
    args: &[String],
) -> Result<Option<MacOsAppLaunch>, String> {
    if os != HostOs::MacOs {
        return Ok(None);
    }
    let Some(macos) = binary
        .parent()
        .filter(|dir| dir.ends_with("Contents/MacOS"))
    else {
        return Ok(None);
    };
    let Some(bundle) = macos.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    if bundle.extension().is_none_or(|ext| ext != "app") {
        return Ok(None);
    }
    let mut profiles = args
        .iter()
        .filter_map(|arg| arg.strip_prefix("--user-data-dir="));
    let profile_dir = profiles.next().filter(|dir| !dir.is_empty());
    if profiles.next().is_some() || !args.iter().any(|arg| arg == OWNERSHIP_FLAG) {
        return Err("browser app launch requires one owned profile".into());
    }
    let profile_dir = profile_dir.ok_or("browser app launch requires an isolated profile")?;
    Ok(Some(MacOsAppLaunch {
        bundle: bundle.into(),
        profile_dir: profile_dir.into(),
        adds_to_recent_items: false,
        creates_new_instance: true,
        activates: false,
    }))
}

pub fn active_port_endpoint(contents: &str) -> Option<String> {
    let mut lines = contents.lines();
    let port = lines
        .next()?
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)?;
    let path = lines.next()?.strip_prefix("/devtools/browser/")?;
    if path.len() != 36 || uuid::Uuid::parse_str(path).is_err() {
        return None;
    }
    Some(format!("ws://127.0.0.1:{port}/devtools/browser/{path}"))
}

#[derive(Clone, Debug, PartialEq)]
pub struct LaunchSpec<'a> {
    pub profile_dir: &'a Path,
    pub width: u32,
    pub height: u32,
    pub initial_url: &'a str,
}

pub fn launch_args(spec: &LaunchSpec<'_>) -> Vec<String> {
    vec![
        OWNERSHIP_FLAG.into(),
        "--headless=new".into(),
        "--remote-debugging-port=0".into(),
        format!("--user-data-dir={}", spec.profile_dir.display()),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-sync".into(),
        "--disable-extensions".into(),
        "--disable-component-extensions-with-background-pages".into(),
        "--disable-background-networking".into(),
        "--password-store=basic".into(),
        "--use-mock-keychain".into(),
        "--disable-features=Translate,CalculateNativeWinOcclusion".into(),
        "--disable-blink-features=AutomationControlled".into(),
        format!("--window-size={},{}", spec.width.max(1), spec.height.max(1)),
        if spec.initial_url.is_empty() {
            "about:blank".into()
        } else {
            spec.initial_url.to_string()
        },
    ]
}

/// Incremental stderr scanner: chunks may split lines anywhere, lines may end in CRLF.
#[derive(Debug, Default)]
pub struct DevToolsListener {
    pending: Vec<u8>,
    tail: VecDeque<String>,
}

impl DevToolsListener {
    pub fn push(&mut self, chunk: &[u8]) -> Option<String> {
        let mut found = None;
        for &byte in chunk {
            if byte == b'\n' {
                let line = std::mem::take(&mut self.pending);
                let endpoint = self.complete_line(&line);
                if found.is_none() {
                    found = endpoint;
                }
            } else if self.pending.len() < MAX_PENDING_LINE {
                self.pending.push(byte);
            }
        }
        found
    }

    /// Stream ended: an unterminated last line still counts.
    pub fn finish(&mut self) -> Option<String> {
        let line = std::mem::take(&mut self.pending);
        self.complete_line(&line)
    }

    pub fn tail(&self) -> String {
        self.tail.iter().cloned().collect::<Vec<_>>().join(" | ")
    }

    fn complete_line(&mut self, raw: &[u8]) -> Option<String> {
        let text = String::from_utf8_lossy(raw);
        let line = text.trim_end_matches('\r').trim();
        if line.is_empty() {
            return None;
        }
        if self.tail.len() == TAIL_LINES {
            self.tail.pop_front();
        }
        self.tail.push_back(line.to_string());
        parse_devtools_line(line)
    }
}

pub fn parse_devtools_line(line: &str) -> Option<String> {
    let start = line.find(DEVTOOLS_MARKER)? + DEVTOOLS_MARKER.len();
    let endpoint = line[start..].split_whitespace().next()?;
    endpoint.starts_with("ws://").then(|| endpoint.to_string())
}

/// The CDP socket grants full control of the browser, so only loopback endpoints are accepted.
pub fn validate_loopback_endpoint(endpoint: &str, schemes: &[&str]) -> Result<url::Url, String> {
    let url = url::Url::parse(endpoint)
        .map_err(|error| format!("invalid endpoint {endpoint}: {error}"))?;
    if !schemes.contains(&url.scheme()) {
        return Err(format!(
            "endpoint {endpoint} must use {}",
            schemes.join(" or ")
        ));
    }
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    };
    if !loopback {
        return Err(format!("endpoint {endpoint} is not a loopback address"));
    }
    Ok(url)
}

#[cfg(test)]
mod tests;
