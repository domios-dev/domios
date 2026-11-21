use super::*;
use std::path::PathBuf;
use std::sync::Mutex;

#[test]
fn settings_document_uses_loopback_token_without_reserved_device_names() {
    let raw = settings_document(43210, "deadbeef01");
    let parsed: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed["statusLine"]["type"], "command");
    assert_eq!(parsed["statusLine"]["enabled"], true);
    let command = parsed["statusLine"]["command"].as_str().unwrap();
    assert!(command.contains("http://127.0.0.1:43210/deadbeef01"));
    assert!(!command.to_ascii_lowercase().contains(" nul"));
}

#[test]
fn http_post_parser_validates_token_content_length_and_chunked_encoding() {
    let token = "tok123";
    let body = br#"{"quota":{"alpha-5h":{"remaining_fraction":0.5}}}"#;
    let plain = format!(
        "POST /{token} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let mut plain_bytes = plain.into_bytes();
    plain_bytes.extend_from_slice(body);
    assert_eq!(
        parse_http_post(&plain_bytes, token).as_deref(),
        Some(body.as_slice())
    );
    assert!(parse_http_post(&plain_bytes, "wrong-token").is_none());

    let chunked = format!(
        "POST /{token} HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
        body.len(),
        std::str::from_utf8(body).unwrap()
    );
    assert_eq!(
        parse_http_post(chunked.as_bytes(), token).as_deref(),
        Some(body.as_slice())
    );
}

#[test]
fn snapshot_formats_four_bucket_statusline_payload_and_strips_email() {
    let now = 1_795_280_000_000_u64;
    let payload = json!({
        "product": "fixture",
        "plan_tier": "Fixture Ultra",
        "email": "private-user@example.com",
        "quota": {
            "3p-5h": { "remaining_fraction": 1, "reset_time": "2026-11-22T02:38:44Z", "reset_in_seconds": 17999 },
            "3p-weekly": { "remaining_fraction": 1, "reset_time": "2026-11-28T21:38:44Z", "reset_in_seconds": 604799 },
            "primary-5h": { "remaining_fraction": 0.9533681, "reset_time": "2026-11-22T02:31:19Z", "reset_in_seconds": 17554 },
            "primary-weekly": { "remaining_fraction": 0.51503307, "reset_time": "2026-11-28T11:31:19Z", "reset_in_seconds": 568354 }
        }
    });
    let quota = parse_statusline_quota(&payload, now).unwrap();
    let snapshot = build_snapshot("acp", "fixture-profile", quota, now);
    assert_eq!(
        snapshot.account_id.as_deref(),
        Some("profile:fixture-profile")
    );
    assert_eq!(snapshot.billing_mode, BillingMode::Subscription);
    assert!(snapshot.limits_reported);
    assert!(snapshot.raw_text.is_empty());
    let details = snapshot.usage_details.unwrap();
    let serialized = details.to_string();
    assert!(!serialized.contains("private-user@example.com"));
    assert_eq!(details["quota"]["title"], "Account quota (Fixture Ultra)");
    let limits = details["quota"]["limits"].as_array().unwrap();
    assert_eq!(limits.len(), 4);
    assert_eq!(limits[0]["label"], "Primary 5h");
    assert_eq!(limits[0]["used_percent"], 4.7);
    assert_eq!(limits[1]["label"], "Primary weekly");
    assert_eq!(limits[1]["used_percent"], 48.5);
    assert_eq!(limits[2]["label"], "3P 5h");
    assert_eq!(limits[2]["used_percent"], 0.0);
    assert_eq!(limits[3]["label"], "3P weekly");
    assert_eq!(limits[3]["used_percent"], 0.0);
}

#[test]
fn probe_fails_fast_on_early_exit_and_cleans_temp_directory() {
    let observed_dir = Arc::new(Mutex::new(None::<PathBuf>));
    let captured_dir = Arc::clone(&observed_dir);
    let started = Instant::now();
    let result = run_command_probe(
        "acp",
        "exit-fixture",
        Duration::from_secs(5),
        1_795_280_000_000,
        move |_settings, cwd| {
            *captured_dir.lock().unwrap() = Some(cwd.to_path_buf());
            #[cfg(windows)]
            {
                let mut cmd = CommandBuilder::new("cmd.exe");
                cmd.args(["/c", "exit 0"]);
                cmd.cwd(cwd);
                cmd
            }
            #[cfg(not(windows))]
            {
                let mut cmd = CommandBuilder::new("sh");
                cmd.args(["-c", "exit 0"]);
                cmd.cwd(cwd);
                cmd
            }
        },
    );
    let err = result.expect_err("exiting without quota must fail closed");
    assert_eq!(err.0, UsageFailureKind::FetchError);
    assert!(started.elapsed() < Duration::from_secs(4));
    let dir = observed_dir.lock().unwrap().clone().unwrap();
    assert!(!dir.exists(), "temp directory must be deleted on exit");
}

#[test]
fn probe_times_out_kills_child_and_cleans_temp_directory() {
    let observed_dir = Arc::new(Mutex::new(None::<PathBuf>));
    let captured_dir = Arc::clone(&observed_dir);
    let started = Instant::now();
    let result = run_command_probe(
        "acp",
        "timeout-fixture",
        Duration::from_millis(350),
        1_795_280_000_000,
        move |_settings, cwd| {
            *captured_dir.lock().unwrap() = Some(cwd.to_path_buf());
            #[cfg(windows)]
            {
                let mut cmd = CommandBuilder::new("cmd.exe");
                cmd.args(["/c", "ping -n 30 127.0.0.1"]);
                cmd.cwd(cwd);
                cmd
            }
            #[cfg(not(windows))]
            {
                let mut cmd = CommandBuilder::new("sh");
                cmd.args(["-c", "sleep 30"]);
                cmd.cwd(cwd);
                cmd
            }
        },
    );
    let err = result.expect_err("silent child must time out");
    assert_eq!(err.0, UsageFailureKind::Timeout);
    assert!(started.elapsed() < Duration::from_secs(5));
    let dir = observed_dir.lock().unwrap().clone().unwrap();
    assert!(!dir.exists(), "temp directory must be deleted on timeout");
}

#[test]
fn probe_captures_quota_over_loopback_kills_child_early_and_cleans_temp_dir() {
    let observed_dir = Arc::new(Mutex::new(None::<PathBuf>));
    let captured_dir = Arc::clone(&observed_dir);
    let poster = Arc::new(Mutex::new(None::<std::thread::JoinHandle<()>>));
    let poster_slot = Arc::clone(&poster);
    let started = Instant::now();
    let now = 1_795_280_000_000_u64;
    let snapshot = run_command_probe(
        "acp",
        "capture-fixture",
        Duration::from_secs(5),
        now,
        move |settings, cwd| {
            *captured_dir.lock().unwrap() = Some(cwd.to_path_buf());
            let raw_settings = std::fs::read_to_string(settings).unwrap();
            let parsed: Value = serde_json::from_str(&raw_settings).unwrap();
            let command = parsed["statusLine"]["command"].as_str().unwrap().to_owned();
            let url = command
                .split_whitespace()
                .find(|part| part.starts_with("http://127.0.0.1:"))
                .unwrap()
                .to_owned();
            let rest = url.strip_prefix("http://").unwrap();
            let (host_port, path) = rest.split_once('/').unwrap();
            let host_port = host_port.to_owned();
            let path = format!("/{path}");
            *poster_slot.lock().unwrap() = Some(std::thread::spawn(move || {
                let body = json!({
                    "plan_tier": "Fixture Ultra",
                    "email": "private-user@example.com",
                    "quota": {
                        "3p-5h": { "remaining_fraction": 1.0, "reset_in_seconds": 17999 },
                        "3p-weekly": { "remaining_fraction": 1.0, "reset_in_seconds": 604799 },
                        "primary-5h": { "remaining_fraction": 0.9533681, "reset_in_seconds": 17554 },
                        "primary-weekly": { "remaining_fraction": 0.51503307, "reset_in_seconds": 568354 }
                    }
                })
                .to_string();
                let mut stream = TcpStream::connect(host_port).unwrap();
                let request = format!(
                    "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(request.as_bytes()).unwrap();
                let _ = stream.flush();
                let mut reply = [0u8; 128];
                let _ = stream.read(&mut reply);
            }));
            #[cfg(windows)]
            {
                let mut cmd = CommandBuilder::new("cmd.exe");
                cmd.args(["/c", "ping -n 30 127.0.0.1"]);
                cmd.cwd(cwd);
                cmd
            }
            #[cfg(not(windows))]
            {
                let mut cmd = CommandBuilder::new("sh");
                cmd.args(["-c", "sleep 30"]);
                cmd.cwd(cwd);
                cmd
            }
        },
    )
    .expect("valid loopback statusLine quota must succeed");

    if let Some(handle) = poster.lock().unwrap().take() {
        handle.join().unwrap();
    }
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "probe must terminate child immediately once quota arrives"
    );
    let dir = observed_dir.lock().unwrap().clone().unwrap();
    assert!(!dir.exists(), "temp directory must be deleted on success");
    assert_eq!(snapshot.account_id.as_deref(), Some("profile:capture-fixture"));
    let details = snapshot.usage_details.unwrap();
    assert!(!details.to_string().contains("private-user@example.com"));
    assert_eq!(details["quota"]["title"], "Account quota (Fixture Ultra)");
    let labels: Vec<&str> = details["quota"]["limits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["label"].as_str().unwrap())
        .collect();
    assert_eq!(
        labels,
        vec!["Primary 5h", "Primary weekly", "3P 5h", "3P weekly"]
    );
}
