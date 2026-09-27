use chrono::TimeZone;

use crate::{
    args::delayed::DelayedAction,
    client::SocketClient,
    cmd::send_text::{guard_against_shell_mangle, resolve_send_text_input},
    resolve_tab,
};

pub fn handle_delayed(client: &mut SocketClient, json: bool, action: DelayedAction) {
    match action {
        DelayedAction::List { tab } => {
            let tab_id = resolve_tab(client, tab);
            let value = request_json(
                "GET",
                &format!("/api/tabs/{}/delayed-messages", tab_id),
                None,
            );
            print_delayed_result(json, value);
        }
        DelayedAction::Create {
            text,
            tab,
            delay,
            at,
            every,
            until,
            count,
            from_file,
            from_pad,
            stdin,
        } => {
            let tab_id = resolve_tab(client, tab);
            let text = required_text(text, from_file, from_pad, stdin);
            let now = now_ms();
            let scheduled_at_ms = parse_start_schedule(delay, at, every.as_deref(), now)
                .unwrap_or_else(|e| exit_error(e));
            let recurrence = parse_recurrence(every, until, count)
                .unwrap_or_else(|e| exit_error(e));
            queue_message(json, &tab_id, text, scheduled_at_ms, recurrence);
        }
        DelayedAction::Edit {
            id,
            text,
            delay,
            at,
            every,
            until,
            count,
            from_file,
            from_pad,
            stdin,
        } => {
            let text = optional_text(text, from_file, from_pad, stdin);
            let scheduled_at_ms = if delay.is_some() || at.is_some() {
                Some(parse_schedule(delay, at, now_ms()).unwrap_or_else(|e| exit_error(e)))
            } else {
                None
            };
            let recurrence = parse_recurrence(every, until, count)
                .unwrap_or_else(|e| exit_error(e));
            if text.is_none() && scheduled_at_ms.is_none() && recurrence.is_none() {
                exit_error(
                    "nothing to update; pass text, --delay/--at, or --every with --until/--count".into(),
                );
            }
            let mut body = serde_json::Map::new();
            if let Some(text) = text {
                body.insert("text".into(), serde_json::json!(text));
            }
            if let Some(scheduled_at_ms) = scheduled_at_ms {
                body.insert("scheduledAtMs".into(), serde_json::json!(scheduled_at_ms));
            }
            if let Some(recurrence) = recurrence {
                body.insert("recurrence".into(), recurrence);
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
    tab: Option<String>,
    text: Option<String>,
    from_file: Option<String>,
    from_pad: Option<String>,
    from_stdin: bool,
    delay: Option<String>,
    at: Option<String>,
    every: Option<String>,
    until: Option<String>,
    count: Option<u32>,
) {
    let tab_id = resolve_tab(client, tab);
    let text = required_text(text, from_file, from_pad, from_stdin);
    let now = now_ms();
    let scheduled_at_ms = parse_start_schedule(delay, at, every.as_deref(), now)
        .unwrap_or_else(|e| exit_error(e));
    let recurrence = parse_recurrence(every, until, count)
        .unwrap_or_else(|e| exit_error(e));
    queue_message(json, &tab_id, text, scheduled_at_ms, recurrence);
}

fn queue_message(
    json: bool,
    tab_id: &str,
    text: String,
    scheduled_at_ms: u64,
    recurrence: Option<serde_json::Value>,
) {
    let mut body = serde_json::json!({
        "text": text,
        "scheduledAtMs": scheduled_at_ms,
    });
    if let Ok(creator) = std::env::var("CODETERM_TAB_ID") {
        if !creator.trim().is_empty() {
            body["createdByTab"] = serde_json::json!(creator);
        }
    }
    if let Some(recurrence) = recurrence {
        body["recurrence"] = recurrence;
    }
    let value = request_json(
        "POST",
        &format!("/api/tabs/{}/delayed-messages", tab_id),
        Some(body),
    );
    print_delayed_result(json, value);
}

pub(crate) fn queue_over_mesh(
    json: bool,
    peer: &str,
    tab_id: &str,
    text: String,
    delay: Option<String>,
    at: Option<String>,
    every: Option<String>,
    until: Option<String>,
    count: Option<u32>,
) {
    let now = now_ms();
    let scheduled_at_ms = parse_start_schedule(delay, at, every.as_deref(), now)
        .unwrap_or_else(|error| exit_error(error));
    let recurrence = parse_recurrence(every, until, count).unwrap_or_else(|error| exit_error(error));
    let mut body = serde_json::json!({
        "text": text,
        "scheduledAtMs": scheduled_at_ms,
    });
    if let Some(recurrence) = recurrence {
        body["recurrence"] = recurrence;
    }
    let path = format!(
        "/api/tabs/{}/delayed-messages",
        urlencoding::encode(tab_id)
    );
    let value = crate::mesh_cli::mesh_relay_fetch_json(peer, "POST", &path, Some(&body.to_string()));
    print_delayed_result(json, value);
}

fn required_text(
    text: Option<String>,
    from_file: Option<String>,
    from_pad: Option<String>,
    from_stdin: bool,
) -> String {
    let mut stdin = std::io::stdin();
    let (text, source) = resolve_send_text_input(text, from_file, from_pad, from_stdin, &mut stdin)
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
    from_pad: Option<String>,
    from_stdin: bool,
) -> Option<String> {
    if text.is_none() && from_file.is_none() && from_pad.is_none() && !from_stdin {
        return None;
    }
    Some(required_text(text, from_file, from_pad, from_stdin))
}

fn request_json(method: &str, path: &str, body: Option<serde_json::Value>) -> serde_json::Value {
    let (base, _) = crate::read_api_config();
    let api = crate::build_local_api().unwrap_or_else(|e| {
        exit_error(format!("failed to connect to local API: {}", e));
    });
    let url = format!("{}{}", base, path);
    let builder = match method {
        "GET" => api.get(&url),
        "POST" => api.post(&url),
        "PATCH" => api.patch(&url),
        "DELETE" => api.delete(&url),
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

pub(crate) fn print_delayed_result(json: bool, value: serde_json::Value) {
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
    let tab = value.get("tabId").and_then(|v| v.as_str()).unwrap_or("?");
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
    println!("{} [{}] tab={} at={} {}", id, state, tab, at, text);
    if let Some(recurrence) = value.get("recurrence") {
        let every = recurrence
            .get("everyMs")
            .and_then(|v| v.as_u64())
            .map(format_duration)
            .unwrap_or_else(|| "?".into());
        let skipped = value
            .get("skippedTicks")
            .and_then(|v| v.as_u64())
            .unwrap_or_default();
        println!("  repeats every {} · next at {} · skipped {}", every, at, skipped);
    }
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

fn parse_start_schedule(
    delay: Option<String>,
    at: Option<String>,
    every: Option<&str>,
    now_ms: u64,
) -> Result<u64, String> {
    if delay.is_none() && at.is_none() {
        if let Some(every) = every {
            let interval = parse_duration_ms(every)?;
            if interval < 60_000 {
                return Err("--every must be at least 1m".into());
            }
            return Ok(now_ms.saturating_add(interval));
        }
    }
    parse_schedule(delay, at, now_ms)
}

fn parse_recurrence(
    every: Option<String>,
    until: Option<String>,
    count: Option<u32>,
) -> Result<Option<serde_json::Value>, String> {
    let Some(every) = every else {
        return if until.is_some() || count.is_some() {
            Err("--until and --count require --every".into())
        } else {
            Ok(None)
        };
    };
    let every_ms = parse_duration_ms(&every)?;
    if every_ms < 60_000 {
        return Err("--every must be at least 1m".into());
    }
    if until.is_some() && count.is_some() {
        return Err("use only one of --until or --count".into());
    }
    if count == Some(0) {
        return Err("--count must be greater than zero".into());
    }
    let until_ms = until.map(|value| parse_absolute_ms(&value)).transpose()?;
    Ok(Some(serde_json::json!({
        "everyMs": every_ms,
        "untilMs": until_ms,
        "count": count,
    })))
}

fn format_duration(ms: u64) -> String {
    if ms % 86_400_000 == 0 {
        format!("{}d", ms / 86_400_000)
    } else if ms % 3_600_000 == 0 {
        format!("{}h", ms / 3_600_000)
    } else if ms % 60_000 == 0 {
        format!("{}m", ms / 60_000)
    } else {
        format!("{}s", ms / 1_000)
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
    use super::{parse_duration_ms, parse_recurrence, parse_schedule, parse_start_schedule};

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

    #[test]
    fn recurrence_schedule_starts_at_the_requested_interval() {
        assert_eq!(
            parse_start_schedule(None, None, Some("5m"), 100).unwrap(),
            300_100
        );
    }

    #[test]
    fn recurrence_parses_count_and_until_as_mutually_exclusive_stops() {
        let count = parse_recurrence(Some("10m".into()), None, Some(3))
            .unwrap()
            .unwrap();
        assert_eq!(count["everyMs"], 600_000);
        assert_eq!(count["count"], 3);
        let until = parse_recurrence(Some("10m".into()), Some("900000".into()), None)
            .unwrap()
            .unwrap();
        assert_eq!(until["untilMs"], 900_000);
        assert!(parse_recurrence(Some("10m".into()), Some("900000".into()), Some(3)).is_err());
        assert!(parse_recurrence(None, None, Some(3)).is_err());
    }

    #[test]
    fn recurrence_rejects_intervals_under_one_minute() {
        assert!(parse_recurrence(Some("59s".into()), None, None).is_err());
    }
}
