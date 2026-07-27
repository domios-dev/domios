use chrono::TimeZone;

use crate::{
    args::delayed::DelayedAction,
    client::SocketClient,
    cmd::send_text::{guard_against_shell_mangle, resolve_send_text_input},
    resolve_pane,
};

pub fn handle_delayed(client: &mut SocketClient, json: bool, action: DelayedAction) {
    match action {
        DelayedAction::List { pane } => {
            let pane_id = resolve_pane(client, pane);
            let value = request_json(
                "GET",
                &format!("/api/panes/{}/delayed-messages", pane_id),
                None,
            );
            print_delayed_result(json, value);
        }
        DelayedAction::Create {
            text,
            pane,
            delay,
            at,
            from_file,
            stdin,
        } => {
            let pane_id = resolve_pane(client, pane);
            let text = required_text(text, from_file, stdin);
            let scheduled_at_ms =
                parse_schedule(delay, at, now_ms()).unwrap_or_else(|e| exit_error(e));
            queue_message(json, &pane_id, text, scheduled_at_ms);
        }
        DelayedAction::Edit {
            id,
            text,
            delay,
            at,
            from_file,
            stdin,
        } => {
            let text = optional_text(text, from_file, stdin);
            let scheduled_at_ms = if delay.is_some() || at.is_some() {
                Some(parse_schedule(delay, at, now_ms()).unwrap_or_else(|e| exit_error(e)))
            } else {
                None
            };
            if text.is_none() && scheduled_at_ms.is_none() {
                exit_error(
                    "nothing to update; pass --text/--from-file/--stdin and/or --delay/--at".into(),
                );
            }
            let mut body = serde_json::Map::new();
            if let Some(text) = text {
                body.insert("text".into(), serde_json::json!(text));
            }
            if let Some(scheduled_at_ms) = scheduled_at_ms {
                body.insert("scheduledAtMs".into(), serde_json::json!(scheduled_at_ms));
            }
            let value = request_json(
                "PATCH",
                &format!("/api/delayed-messages/{}", id),
                Some(serde_json::Value::Object(body)),
            );
            print_delayed_result(json, value);
        }
        DelayedAction::Delete { id } => {
            let value = request_json("DELETE", &format!("/api/delayed-messages/{}", id), None);
            print_delayed_result(json, value);
        }
        DelayedAction::SendNow { id } => {
            let value = request_json(
                "POST",
                &format!("/api/delayed-messages/{}/send-now", id),
                None,
            );
            print_delayed_result(json, value);
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "CLI forwarding mirrors send text options."
)]
pub fn queue_from_send(
    client: &mut SocketClient,
    json: bool,
    pane: Option<String>,
    text: Option<String>,
    from_file: Option<String>,
    from_stdin: bool,
    delay: Option<String>,
    at: Option<String>,
) {
    let pane_id = resolve_pane(client, pane);
    let text = required_text(text, from_file, from_stdin);
    let scheduled_at_ms = parse_schedule(delay, at, now_ms()).unwrap_or_else(|e| exit_error(e));
    queue_message(json, &pane_id, text, scheduled_at_ms);
}

fn queue_message(json: bool, pane_id: &str, text: String, scheduled_at_ms: u64) {
    let mut body = serde_json::json!({
        "text": text,
        "scheduledAtMs": scheduled_at_ms,
    });
    if let Ok(creator) = std::env::var("CODETERM_PANE_ID") {
        if !creator.trim().is_empty() {
            body["createdByPane"] = serde_json::json!(creator);
        }
    }
    let value = request_json(
        "POST",
        &format!("/api/panes/{}/delayed-messages", pane_id),
        Some(body),
    );
    print_delayed_result(json, value);
}

fn required_text(text: Option<String>, from_file: Option<String>, from_stdin: bool) -> String {
    let mut stdin = std::io::stdin();
    let (text, source) = resolve_send_text_input(text, from_file, from_stdin, &mut stdin)
        .unwrap_or_else(|e| exit_error(e));
    if text.trim().is_empty() {
        exit_error(format!(
            "delayed message payload is empty (source: {})",
            source
        ));
    }
    guard_against_shell_mangle(&text).unwrap_or_else(|e| exit_error(e));
    text
}

fn optional_text(
    text: Option<String>,
    from_file: Option<String>,
    from_stdin: bool,
) -> Option<String> {
    if text.is_none() && from_file.is_none() && !from_stdin {
        return None;
    }
    Some(required_text(text, from_file, from_stdin))
}

fn request_json(method: &str, path: &str, body: Option<serde_json::Value>) -> serde_json::Value {
    let (base, token) = crate::read_api_config();
    let http = crate::build_http(&token).unwrap_or_else(|e| {
        exit_error(format!("failed to build HTTP client: {}", e));
    });
    let url = format!("{}{}", base, path);
    let builder = match method {
        "GET" => http.get(&url),
        "POST" => http.post(&url),
        "PATCH" => http.patch(&url),
        "DELETE" => http.delete(&url),
        _ => exit_error(format!("unsupported method: {}", method)),
    };
    let builder = match body {
        Some(body) => builder.json(&body),
        None => builder,
    };
    let resp = builder.send().unwrap_or_else(|e| {
        exit_error(format!("request failed: {}", e));
    });
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        exit_error(format!("HTTP {}: {}", status, text.trim()));
    }
    if text.trim().is_empty() {
        return serde_json::json!({"ok": true});
    }
    serde_json::from_str(&text).unwrap_or_else(|e| {
        exit_error(format!("invalid JSON response: {}", e));
    })
}

fn print_delayed_result(json: bool, value: serde_json::Value) {
    if json {
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }
    if let Some(arr) = value.as_array() {
        if arr.is_empty() {
            println!("(no delayed messages)");
            return;
        }
        for item in arr {
            print_delayed_item(item);
        }
        return;
    }
    if value.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        println!("OK");
        return;
    }
    print_delayed_item(&value);
}

fn print_delayed_item(value: &serde_json::Value) {
    let id = value.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let pane = value.get("paneId").and_then(|v| v.as_str()).unwrap_or("?");
    let state = value.get("state").and_then(|v| v.as_str()).unwrap_or("?");
    let at = value
        .get("scheduledAtMs")
        .and_then(|v| v.as_u64())
        .map(format_ms)
        .unwrap_or_else(|| "?".into());
    let text = value
        .get("text")
        .and_then(|v| v.as_str())
        .map(shorten)
        .unwrap_or_default();
    println!("{} [{}] tab={} at={} {}", id, state, pane, at, text);
    if value
        .get("sessionChanged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        println!("  warning: Tab session changed since scheduling");
    }
    if let Some(error) = value.get("error").and_then(|v| v.as_str()) {
        if !error.is_empty() {
            println!("  error: {}", error);
        }
    }
}

fn shorten(text: &str) -> String {
    let single = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated: String = single.chars().take(90).collect();
    if single.chars().count() > 90 {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

fn format_ms(ms: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms as i64)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| ms.to_string())
}

pub fn parse_schedule(
    delay: Option<String>,
    at: Option<String>,
    now_ms: u64,
) -> Result<u64, String> {
    match (delay, at) {
        (Some(_), Some(_)) => Err("use only one of --delay or --at".into()),
        (Some(delay), None) => {
            let delta = parse_duration_ms(&delay)?;
            if delta == 0 {
                return Err("--delay must be greater than zero".into());
            }
            Ok(now_ms.saturating_add(delta))
        }
        (None, Some(at)) => parse_absolute_ms(&at),
        (None, None) => Err("pass --delay or --at".into()),
    }
}

fn parse_absolute_ms(value: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("--at is empty".into());
    }
    if let Ok(ms) = trimmed.parse::<u64>() {
        return Ok(ms);
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return Ok(dt.timestamp_millis() as u64);
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M") {
        let local = chrono::Local
            .from_local_datetime(&naive)
            .single()
            .ok_or_else(|| format!("ambiguous local time: {}", trimmed))?;
        return Ok(local.timestamp_millis() as u64);
    }
    Err(format!(
        "invalid --at value {}; use unix milliseconds, RFC3339, or local YYYY-MM-DDTHH:MM",
        trimmed
    ))
}

fn parse_duration_ms(value: &str) -> Result<u64, String> {
    let s = value.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Err("--delay is empty".into());
    }
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s
            .parse::<u64>()
            .map(|minutes| minutes.saturating_mul(60_000))
            .map_err(|e| e.to_string());
    }

    let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    let mut i = 0usize;
    let mut total = 0u64;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            return Err(format!("invalid --delay value: {}", value));
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let amount: u64 = chars[start..i]
            .iter()
            .collect::<String>()
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        if i >= chars.len() {
            return Err(format!("missing unit in --delay value: {}", value));
        }
        let unit = chars[i];
        i += 1;
        let factor = match unit {
            's' => 1_000,
            'm' => 60_000,
            'h' => 3_600_000,
            'd' => 86_400_000,
            _ => return Err(format!("unsupported --delay unit: {}", unit)),
        };
        total = total.saturating_add(amount.saturating_mul(factor));
    }
    Ok(total)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn exit_error(msg: String) -> ! {
    eprintln!("Error: {}", msg);
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::{parse_duration_ms, parse_schedule};

    #[test]
    fn delay_bare_number_means_minutes() {
        assert_eq!(parse_duration_ms("5").unwrap(), 300_000);
    }

    #[test]
    fn delay_accepts_compound_units() {
        assert_eq!(parse_duration_ms("1h30m45s").unwrap(), 5_445_000);
    }

    #[test]
    fn schedule_requires_delay_or_at() {
        assert!(parse_schedule(None, None, 100).is_err());
    }

    #[test]
    fn schedule_adds_delay_to_now() {
        assert_eq!(
            parse_schedule(Some("2m".into()), None, 100).unwrap(),
            120_100
        );
    }
}
