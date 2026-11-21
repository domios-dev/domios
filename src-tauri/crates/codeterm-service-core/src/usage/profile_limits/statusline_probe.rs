use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use codeterm_plugin::agent_runtime::limits_source;
use codeterm_plugin::js_runtime::{probe_bin, BinResolution};
use codeterm_usage::contract::BillingMode;
use portable_pty::{cmdbuilder::WindowsJobPolicy, native_pty_system, CommandBuilder, PtySize};
use serde_json::{json, Value};

use crate::agent::usage_parser::UsageSnapshot;
use crate::service::agent_runtime::projections::quota_feed::parse_statusline_quota;
use crate::usage::store::UsageFailureKind;

pub(super) const READER_ID: &str = "statusline-quota";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const EXIT_GRACE: Duration = Duration::from_millis(250);
const SOCKET_READ_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_REQUEST_BYTES: usize = 64 * 1024;
const HTTP_NO_CONTENT: &[u8] =
    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

enum ProbeSignal {
    Quota(Value),
    Exited,
}

fn capture_command(port: u16, token: &str) -> String {
    #[cfg(windows)]
    {
        format!(
            "curl.exe -s --max-time 2 -X POST -H Expect: --data-binary @- http://127.0.0.1:{port}/{token}"
        )
    }
    #[cfg(not(windows))]
    {
        format!(
            "curl -s --max-time 2 -o /dev/null -X POST -H Expect: --data-binary @- http://127.0.0.1:{port}/{token}"
        )
    }
}

fn settings_document(port: u16, token: &str) -> String {
    json!({
        "statusLine": {
            "type": "command",
            "enabled": true,
            "command": capture_command(port, token),
        }
    })
    .to_string()
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|pos| pos + 4)
}

fn parse_headers(header_bytes: &[u8]) -> Option<(String, Option<usize>, bool)> {
    let text = std::str::from_utf8(header_bytes).ok()?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next()?.to_owned();
    let mut content_length = None;
    let mut chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value.trim().parse::<usize>().ok()?;
            if parsed > MAX_REQUEST_BYTES {
                return None;
            }
            content_length = Some(parsed);
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        }
    }
    Some((request_line, content_length, chunked))
}

fn decode_chunked(raw: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while cursor < raw.len() {
        let rel = raw[cursor..].windows(2).position(|w| w == b"\r\n")?;
        let size_line = std::str::from_utf8(&raw[cursor..cursor + rel]).ok()?;
        let hex = size_line.split(';').next()?.trim();
        let size = usize::from_str_radix(hex, 16).ok()?;
        cursor += rel + 2;
        if size == 0 {
            return Some(out);
        }
        if out.len().saturating_add(size) > MAX_REQUEST_BYTES || cursor + size + 2 > raw.len() {
            return None;
        }
        out.extend_from_slice(&raw[cursor..cursor + size]);
        cursor += size;
        if &raw[cursor..cursor + 2] != b"\r\n" {
            return None;
        }
        cursor += 2;
    }
    None
}

fn parse_http_post(raw: &[u8], expected_token: &str) -> Option<Vec<u8>> {
    let header_end = find_header_end(raw)?;
    let (request_line, content_length, chunked) = parse_headers(&raw[..header_end])?;
    let expected_prefix = format!("POST /{expected_token} HTTP/1.");
    if !request_line.starts_with(&expected_prefix) {
        return None;
    }
    let body = &raw[header_end..];
    if chunked {
        decode_chunked(body)
    } else if let Some(len) = content_length {
        (body.len() >= len).then(|| body[..len].to_vec())
    } else {
        (!body.is_empty()).then(|| body.to_vec())
    }
}

fn request_complete(buf: &[u8]) -> bool {
    let Some(header_end) = find_header_end(buf) else {
        return false;
    };
    let Some((_, content_length, chunked)) = parse_headers(&buf[..header_end]) else {
        return true;
    };
    let body = &buf[header_end..];
    if chunked {
        body.windows(5).any(|w| w == b"0\r\n\r\n")
    } else if let Some(len) = content_length {
        body.len() >= len
    } else {
        false
    }
}

fn read_http_request(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let _ = stream.set_read_timeout(Some(SOCKET_READ_TIMEOUT));
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len().saturating_add(n) > MAX_REQUEST_BYTES {
                    let _ = stream.write_all(HTTP_NO_CONTENT);
                    return None;
                }
                buf.extend_from_slice(&chunk[..n]);
                if request_complete(&buf) {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = stream.write_all(HTTP_NO_CONTENT);
    let _ = stream.flush();
    Some(buf)
}

fn build_snapshot(provider: &str, profile: &str, quota: Value, now_ms: u64) -> UsageSnapshot {
    UsageSnapshot {
        usage_details: Some(json!({
            "limits": [],
            "dollar_credits": [],
            "extra_usage": null,
            "quota": quota,
        })),
        provider: provider.to_owned(),
        billing_mode: BillingMode::Subscription,
        limits_reported: true,
        account_id: Some(limits_source::balance_account_key(profile)),
        captured_at_ms: now_ms as i64,
        session_pct: None,
        session_resets_at_ms: None,
        weekly_pct: None,
        weekly_resets_at_ms: None,
        spend_cents: None,
        spend_limit_cents: None,
        spend_remaining_cents: None,
        spend_resets_at_ms: None,
        raw_text: String::new(),
    }
}

fn run_command_probe(
    provider: &str,
    profile: &str,
    timeout: Duration,
    now_ms: u64,
    build_command: impl FnOnce(&Path, &Path) -> CommandBuilder,
) -> Result<UsageSnapshot, (UsageFailureKind, String)> {
    let temp_dir = tempfile::Builder::new()
        .prefix("domios-quota-probe-")
        .tempdir()
        .map_err(|error| {
            (
                UsageFailureKind::Internal,
                format!("Failed to prepare temporary quota probe directory: {error}"),
            )
        })?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| {
        (
            UsageFailureKind::Internal,
            format!("Failed to bind local quota probe listener: {error}"),
        )
    })?;
    let addr = listener.local_addr().map_err(|error| {
        (
            UsageFailureKind::Internal,
            format!("Failed to inspect local quota probe listener: {error}"),
        )
    })?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let settings_path = temp_dir.path().join("settings.json");
    std::fs::write(&settings_path, settings_document(addr.port(), &token)).map_err(|error| {
        (
            UsageFailureKind::Internal,
            format!("Failed to write temporary status line settings: {error}"),
        )
    })?;

    let (tx, rx) = mpsc::channel::<ProbeSignal>();
    let stop = Arc::new(AtomicBool::new(false));
    let listener_done = Arc::new(AtomicBool::new(false));
    let listener_stop = Arc::clone(&stop);
    let listener_finished = Arc::clone(&listener_done);
    let listener_tx = tx.clone();
    let listener_token = token.clone();
    let listener_handle = std::thread::spawn(move || {
        while let Ok((mut stream, _)) = listener.accept() {
            if listener_stop.load(Ordering::Acquire) {
                break;
            }
            let Some(raw) = read_http_request(&mut stream) else {
                continue;
            };
            let Some(body) = parse_http_post(&raw, &listener_token) else {
                continue;
            };
            let Ok(payload) = serde_json::from_slice::<Value>(&body) else {
                continue;
            };
            let Some(quota) = parse_statusline_quota(&payload, now_ms) else {
                continue;
            };
            if quota
                .get("limits")
                .and_then(Value::as_array)
                .is_some_and(|limits| !limits.is_empty())
            {
                let _ = listener_tx.send(ProbeSignal::Quota(quota));
                break;
            }
        }
        listener_finished.store(true, Ordering::Release);
    });
    struct ListenerGuard {
        stop: Arc<AtomicBool>,
        done: Arc<AtomicBool>,
        addr: std::net::SocketAddr,
        handle: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for ListenerGuard {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if !self.done.load(Ordering::Acquire) {
                let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(100));
            }
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }
    let listener_guard = ListenerGuard {
        stop,
        done: listener_done,
        addr,
        handle: Some(listener_handle),
    };

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| {
            (
                UsageFailureKind::Internal,
                format!("Failed to open headless PTY for quota probe: {error}"),
            )
        })?;
    let mut cmd = build_command(&settings_path, temp_dir.path());
    cmd.set_windows_job_policy(WindowsJobPolicy::Contained);
    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(child) => child,
        Err(error) => {
            drop(pair);
            return Err((
                UsageFailureKind::FetchError,
                format!("Failed to launch quota probe binary: {error}"),
            ));
        }
    };
    drop(pair.slave);

    let mut killer = child.clone_killer();
    let exit_tx = tx;
    let wait_handle = std::thread::spawn(move || {
        let _ = child.wait();
        let _ = exit_tx.send(ProbeSignal::Exited);
    });
    let drain_handle = match pair.master.try_clone_reader() {
        Ok(mut reader) => Some(std::thread::spawn(move || {
            let mut sink = [0u8; 4096];
            loop {
                match reader.read(&mut sink) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        })),
        Err(_) => None,
    };

    let deadline = Instant::now() + timeout;
    let outcome = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break Err((
                UsageFailureKind::Timeout,
                String::from("Status line quota probe timed out before receiving quota"),
            ));
        }
        match rx.recv_timeout(remaining) {
            Ok(ProbeSignal::Quota(quota)) => break Ok(quota),
            Ok(ProbeSignal::Exited) => {
                let grace = remaining.min(EXIT_GRACE);
                match rx.recv_timeout(grace) {
                    Ok(ProbeSignal::Quota(quota)) => break Ok(quota),
                    _ => {
                        break Err((
                            UsageFailureKind::FetchError,
                            String::from(
                                "Status line quota probe exited before reporting model quota",
                            ),
                        ));
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                break Err((
                    UsageFailureKind::Timeout,
                    String::from("Status line quota probe timed out before receiving quota"),
                ));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Err((
                    UsageFailureKind::FetchError,
                    String::from("Status line quota probe ended without reporting model quota"),
                ));
            }
        }
    };

    let _ = killer.kill();
    let _ = wait_handle.join();
    drop(pair.master);
    if let Some(handle) = drain_handle {
        let _ = handle.join();
    }
    drop(listener_guard);
    drop(temp_dir);

    outcome.map(|quota| build_snapshot(provider, profile, quota, now_ms))
}

pub(super) fn read_profile(
    provider: &str,
    profile: &str,
    binary: &str,
    env: &BTreeMap<String, String>,
) -> Result<UsageSnapshot, (UsageFailureKind, String)> {
    let resolved = match probe_bin(binary).resolution {
        BinResolution::Found(path) => path,
        BinResolution::Missing => {
            return Err((
                UsageFailureKind::SourceUnavailable,
                format!("Quota probe binary '{binary}' was not found on PATH"),
            ));
        }
        BinResolution::Unknown => {
            return Err((
                UsageFailureKind::SourceUnavailable,
                format!("Quota probe binary '{binary}' availability is unknown"),
            ));
        }
    };
    let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    run_command_probe(
        provider,
        profile,
        PROBE_TIMEOUT,
        now_ms,
        |settings_path, cwd| {
            let mut cmd = CommandBuilder::new(&resolved);
            cmd.arg("--settings");
            cmd.arg(settings_path);
            cmd.cwd(cwd);
            cmd.env("TERM", "xterm-256color");
            for (key, value) in env {
                cmd.env(key, value);
            }
            cmd
        },
    )
}

#[cfg(test)]
mod tests;
