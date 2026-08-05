use crate::{build_local_api, read_api_config, PasteAction};

pub fn handle_paste(json: bool, action: PasteAction) {
    let (base, _) = read_api_config();
    let http = match build_local_api() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: failed to build HTTP client: {}", e);
            std::process::exit(1);
        }
    };
    match action {
        PasteAction::Put {
            body,
            from_file,
            from_pad,
            from_stdin,
            ttl,
            content_type,
            pop,
        } => {
            let resolved_body = if let Some(path) = from_pad {
                crate::cmd::payload_source::read_from_pad(&path).unwrap_or_else(|e| {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                })
            } else if from_stdin {
                read_stdin_or_die("--from-stdin")
            } else if let Some(path) = from_file {
                match std::fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error: failed to read --from-file {}: {}", path, e);
                        std::process::exit(1);
                    }
                }
            } else {
                body.unwrap_or_default()
            };
            if resolved_body.trim().is_empty() {
                eprintln!("Error: paste body is empty (positional, --from-file, and --from-stdin all yielded nothing).");
                eprintln!(
                    "hint: pipe via heredoc — codeterm paste put --from-stdin <<'EOF' ... EOF"
                );
                std::process::exit(2);
            }
            let mut payload = serde_json::json!({ "body": resolved_body });
            if let Some(ms) = ttl.and_then(|s| parse_duration_ms(&s)) {
                payload["ttl_ms"] = serde_json::json!(ms);
            }
            if let Some(ct) = content_type {
                payload["content_type"] = serde_json::json!(ct);
            }
            if pop {
                payload["pop_on_read"] = serde_json::json!(true);
            }
            match http
                .post(format!("{}/api/pasteboard", base))
                .json(&payload)
                .send()
            {
                Ok(resp) => {
                    let status = resp.status();
                    let val: serde_json::Value = resp.json().unwrap_or_default();
                    if !status.is_success() {
                        let msg = val
                            .get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or("paste put failed");
                        eprintln!("Error: HTTP {}: {}", status.as_u16(), msg);
                        std::process::exit(1);
                    }
                    if json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else if let Some(id) = val.get("id").and_then(|v| v.as_str()) {
                        println!("{}", id);
                    } else {
                        eprintln!("Error: server returned no paste id");
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        PasteAction::Get { id, pop } => fetch_paste(&http, &base, &id, pop, json),
        PasteAction::List => match http.get(format!("{}/api/pasteboard", base)).send() {
            Ok(resp) => {
                let val: serde_json::Value = resp.json().unwrap_or_default();
                if json {
                    println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    return;
                }
                let Some(arr) = val.as_array() else {
                    eprintln!("Unexpected response shape");
                    std::process::exit(1);
                };
                if arr.is_empty() {
                    println!("No pastes.");
                    return;
                }
                println!(
                    "{:<14}  {:>8}  {:>9}  content_type",
                    "id", "size", "ttl_left"
                );
                for entry in arr {
                    let id = entry.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                    let size = entry.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                    let ttl_left_ms = entry
                        .get("ttl_remaining_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let ct = entry
                        .get("content_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    println!(
                        "{:<14}  {:>8}  {:>9}  {}",
                        id,
                        size,
                        format_ms(ttl_left_ms),
                        ct,
                    );
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        },
        PasteAction::Rm { id } => {
            match http
                .delete(format!("{}/api/pasteboard/{}", base, id))
                .send()
            {
                Ok(resp) => {
                    if resp.status().is_success() {
                        println!("Deleted: {}", id);
                    } else {
                        eprintln!("Error: HTTP {}", resp.status().as_u16());
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

fn fetch_paste(
    http: &crate::local_api::LocalApiClient,
    base: &str,
    id: &str,
    pop: bool,
    json: bool,
) {
    let url = if pop {
        format!("{}/api/pasteboard/{}?pop=true", base, id)
    } else {
        format!("{}/api/pasteboard/{}", base, id)
    };
    match http.get(&url).send() {
        Ok(resp) => {
            let status = resp.status();
            if status.as_u16() == 404 {
                eprintln!("Error: paste not found: {}", id);
                std::process::exit(1);
            }
            let val: serde_json::Value = resp.json().unwrap_or_default();
            if !status.is_success() {
                eprintln!("Error: HTTP {}", status.as_u16());
                std::process::exit(1);
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&val).unwrap());
            } else if let Some(body) = val.get("body").and_then(|v| v.as_str()) {
                print!("{}", body);
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn read_stdin_or_die(source: &str) -> String {
    use std::io::Read;
    let mut buf = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
        eprintln!("Error: failed to read {}: {}", source, e);
        std::process::exit(1);
    }
    buf
}

/// Parse a short duration spec like "30m", "1h", "1h30m", "24h", "90s" into
/// milliseconds. Returns None if the input is unparseable so the caller falls
/// back to the server default. Shared with the mesh-relay arg map so both
/// paths interpret `--ttl` the same way.
pub fn parse_duration_ms(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut total: u64 = 0;
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n: u64 = num.parse().ok()?;
            num.clear();
            total += match c {
                's' => n * 1_000,
                'm' => n * 60 * 1_000,
                'h' => n * 60 * 60 * 1_000,
                'd' => n * 24 * 60 * 60 * 1_000,
                _ => return None,
            };
        }
    }
    if !num.is_empty() {
        // Bare number = minutes, matching the cron CLI convention.
        let n: u64 = num.parse().ok()?;
        total += n * 60 * 1_000;
    }
    if total == 0 {
        None
    } else {
        Some(total)
    }
}

fn format_ms(ms: u64) -> String {
    let secs = ms / 1000;
    if secs >= 3600 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        format!("{}h{}m", h, m)
    } else if secs >= 60 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_handles_known_suffixes() {
        assert_eq!(parse_duration_ms("30m"), Some(30 * 60 * 1000));
        assert_eq!(parse_duration_ms("1h"), Some(3_600_000));
        assert_eq!(parse_duration_ms("1h30m"), Some(90 * 60 * 1000));
        assert_eq!(parse_duration_ms("24h"), Some(24 * 3_600_000));
        assert_eq!(parse_duration_ms("90s"), Some(90_000));
    }

    #[test]
    fn parse_duration_bare_number_is_minutes() {
        assert_eq!(parse_duration_ms("15"), Some(15 * 60 * 1000));
    }

    #[test]
    fn parse_duration_rejects_garbage() {
        assert!(parse_duration_ms("").is_none());
        assert!(parse_duration_ms("abc").is_none());
        assert!(parse_duration_ms("1z").is_none());
        assert!(parse_duration_ms("0s").is_none());
    }
}
