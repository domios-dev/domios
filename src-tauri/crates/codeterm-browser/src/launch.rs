use std::collections::VecDeque;
use std::path::Path;

pub const OWNERSHIP_FLAG: &str = "--codeterm-owned-browser";

const DEVTOOLS_MARKER: &str = "DevTools listening on ";
const MAX_PENDING_LINE: usize = 64 * 1024;
const TAIL_LINES: usize = 8;

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
mod tests {
    use super::*;

    const READY: &str =
        "DevTools listening on ws://127.0.0.1:53817/devtools/browser/5c1a3b9e-8f0d-4f8e-9f41-2b1c0e6d7a10";

    #[test]
    fn browser_launch_args_carry_hygiene_flags_and_profile() {
        let profile = Path::new("/data/browsers/sessions/abc");
        let args = launch_args(&LaunchSpec {
            profile_dir: profile,
            width: 1600,
            height: 900,
            initial_url: "",
        });
        for flag in [
            "--headless=new",
            "--remote-debugging-port=0",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-sync",
            "--disable-extensions",
            "--disable-component-extensions-with-background-pages",
            "--disable-background-networking",
            "--password-store=basic",
            "--use-mock-keychain",
            "--disable-features=Translate,CalculateNativeWinOcclusion",
            "--disable-blink-features=AutomationControlled",
            "--window-size=1600,900",
        ] {
            assert!(args.iter().any(|arg| arg == flag), "missing {flag}");
        }
        assert!(args.contains(&format!("--user-data-dir={}", profile.display())));
        assert_eq!(args.last().map(String::as_str), Some("about:blank"));
    }

    #[test]
    fn browser_launch_args_end_with_initial_url_when_given() {
        let args = launch_args(&LaunchSpec {
            profile_dir: Path::new("p"),
            width: 0,
            height: 0,
            initial_url: "http://127.0.0.1:5173/",
        });
        assert_eq!(
            args.last().map(String::as_str),
            Some("http://127.0.0.1:5173/")
        );
        assert!(args.iter().any(|arg| arg == "--window-size=1,1"));
    }

    #[test]
    fn browser_devtools_line_parses_endpoint() {
        assert_eq!(
            parse_devtools_line(READY).as_deref(),
            Some("ws://127.0.0.1:53817/devtools/browser/5c1a3b9e-8f0d-4f8e-9f41-2b1c0e6d7a10")
        );
        assert_eq!(parse_devtools_line("DevTools listening on http://x"), None);
        assert_eq!(
            parse_devtools_line("[1008/101010.123:ERROR:gpu] nope"),
            None
        );
    }

    #[test]
    fn browser_devtools_listener_finds_endpoint_among_real_stderr_noise() {
        let mut listener = DevToolsListener::default();
        let stderr = format!(
            "[24312:23440:1008/120001.512:ERROR:cert_verify_proc_builtin.cc(1052)] CertVerifyProcBuiltin failed\r\n\r\n{READY}\r\n[24312:1008/120001.700:WARNING:gpu_init.cc(456)] fallback\r\n"
        );
        assert_eq!(
            listener.push(stderr.as_bytes()).as_deref(),
            parse_devtools_line(READY).as_deref()
        );
    }

    #[test]
    fn browser_devtools_listener_joins_partial_chunks() {
        let mut listener = DevToolsListener::default();
        let bytes = format!("noise line\n{READY}\r\n");
        let (a, rest) = bytes.as_bytes().split_at(17);
        let (b, c) = rest.split_at(30);
        assert_eq!(listener.push(a), None);
        assert_eq!(listener.push(b), None);
        assert!(listener.push(c).unwrap().ends_with("2b1c0e6d7a10"));
    }

    #[test]
    fn browser_devtools_listener_strips_crlf_split_between_chunks() {
        let mut listener = DevToolsListener::default();
        assert_eq!(listener.push(format!("{READY}\r").as_bytes()), None);
        assert!(listener.push(b"\n").unwrap().ends_with("2b1c0e6d7a10"));
    }

    #[test]
    fn browser_devtools_listener_accepts_unterminated_last_line_on_finish() {
        let mut listener = DevToolsListener::default();
        assert_eq!(listener.push(READY.as_bytes()), None);
        assert!(listener.finish().is_some());
    }

    #[test]
    fn browser_devtools_listener_keeps_bounded_tail_for_diagnostics() {
        let mut listener = DevToolsListener::default();
        for index in 0..20 {
            listener.push(format!("line {index}\r\n").as_bytes());
        }
        let tail = listener.tail();
        assert!(tail.starts_with("line 12"));
        assert!(tail.ends_with("line 19"));
    }

    #[test]
    fn browser_devtools_listener_bounds_an_endless_line() {
        let mut listener = DevToolsListener::default();
        listener.push(&vec![b'x'; MAX_PENDING_LINE * 2]);
        assert_eq!(listener.pending.len(), MAX_PENDING_LINE);
    }

    #[test]
    fn browser_endpoint_validation_accepts_only_loopback() {
        assert!(
            validate_loopback_endpoint("ws://127.0.0.1:9222/devtools/browser/x", &["ws"]).is_ok()
        );
        assert!(validate_loopback_endpoint("ws://localhost:9222/x", &["ws"]).is_ok());
        assert!(validate_loopback_endpoint("ws://[::1]:9222/x", &["ws"]).is_ok());
        assert!(validate_loopback_endpoint("ws://192.168.1.4:9222/x", &["ws"]).is_err());
        assert!(validate_loopback_endpoint("wss://127.0.0.1:9222/x", &["ws"]).is_err());
        assert!(validate_loopback_endpoint("http://127.0.0.1:9223", &["http"]).is_ok());
    }
}
