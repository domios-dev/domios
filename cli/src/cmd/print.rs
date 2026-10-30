use std::io::Write;

use crate::client::SocketCallError;
use crate::cmd::error_contract::{json_mode, ErrorCode};
use crate::cmd::plan_output::error_envelope_with_data;
mod neighbor_refusal;

/// Exit status of every failed CLI command, with and without `--json`.
pub const CLI_ERROR_EXIT_CODE: i32 = 1;

const UNEXPLAINED_RPC_FAILURE: &str = "the daemon rejected this command without an explanation";

/// A CLI failure with human-readable text and machine-readable RPC metadata.
/// Structured daemon data stays intact through the `--json` error envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    pub message: String,
    pub code: Option<i32>,
    pub data: Option<serde_json::Value>,
}

impl CliError {
    pub fn new(message: impl Into<String>) -> Self {
        let message = message.into();
        if let Some((code, text)) = crate::client::parse_rpc_display(&message) {
            return Self {
                message: rpc_explanation(text),
                code: Some(code),
                data: None,
            };
        }
        if let Some((status, body)) = parse_http_line(&message) {
            return Self::http(status, body);
        }
        Self {
            message,
            code: None,
            data: None,
        }
    }

    /// An HTTP refusal keeps its status and structured body so the envelope can
    /// classify it (404 not found, 409 conflict) instead of echoing raw text.
    pub fn http(status: u16, body: &str) -> Self {
        let trimmed = body.trim();
        let parsed = serde_json::from_str::<serde_json::Value>(trimmed)
            .ok()
            .filter(serde_json::Value::is_object);
        let detail = parsed
            .as_ref()
            .and_then(|value| {
                ["error", "message", "reason"]
                    .iter()
                    .find_map(|key| value.get(*key).and_then(serde_json::Value::as_str))
            })
            .unwrap_or(trimmed)
            .to_string();
        let refusal = parsed.as_ref().and_then(|value| value.get("failure"));
        let typed_refusal = refusal.and_then(render_account_mesh_refusal);
        let mut data = parsed.unwrap_or_else(|| {
            if trimmed.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::json!({ "body": trimmed })
            }
        });
        data["http_status"] = serde_json::json!(status);
        let detail = match data.get("code").and_then(serde_json::Value::as_str) {
            Some(code) => format!("{code}: {detail}"),
            None => detail,
        };
        let startup = data.get("startup").is_some()
            && matches!(
                data.pointer("/error/code")
                    .and_then(serde_json::Value::as_str),
                Some("starting" | "startup_failed")
            );
        let message = if startup {
            detail
        } else if let Some((message, hint)) = typed_refusal {
            data["hint"] = serde_json::json!(hint);
            format!("✗ {message}\nfix: {hint}")
        } else if detail.is_empty() {
            format!("HTTP {status}")
        } else {
            format!("HTTP {status}: {detail}")
        };
        Self {
            message,
            code: None,
            data: Some(data),
        }
    }

    /// A clap usage failure: always an invalid-argument error, whatever its text.
    pub fn usage(error: &clap::Error) -> Self {
        let rendered = error.render().to_string();
        let trimmed = rendered.trim();
        let body = trimmed.strip_prefix("error: ").unwrap_or(trimmed);
        let message = body.split("\n\n").next().unwrap_or(body).trim().to_string();
        Self {
            message,
            code: None,
            data: Some(serde_json::json!({ "code": ErrorCode::InvalidArgument.as_str() })),
        }
    }
}

fn render_account_mesh_refusal(value: &serde_json::Value) -> Option<(String, String)> {
    let reason = value.get("reason")?.as_str()?;
    let (message, hint) = match reason {
        "unauthenticated" | "signed-out" => (
            "Your Domios account session could not be verified.",
            "sign in again, then retry domios mesh link <installation-id>",
        ),
        "verification-unavailable" => (
            "Your Domios account membership could not be verified.",
            "sign in again, then retry domios mesh link <installation-id>",
        ),
        "app-check" | "forbidden" => (
            "The account service refused this machine link.",
            "sign in again and confirm this machine is allowed, then retry domios mesh link <installation-id>",
        ),
        "invalid" => (
            "The account service rejected the machine link details.",
            "refresh the account machine list, then retry domios mesh link <installation-id>",
        ),
        "authority-unavailable" | "unavailable" => (
            "The account service is temporarily unavailable.",
            "retry domios mesh link <installation-id> in a moment",
        ),
        "offline" => (
            "The other machine is offline.",
            "bring both machines online, then retry domios mesh link <installation-id>",
        ),
        "peer-unreachable" => (
            "The other machine could not be reached.",
            "bring both machines online, then retry domios mesh link <installation-id>",
        ),
        "handshake-failed" => (
            "The machines could not complete the link handshake.",
            "make sure both machines are online, then retry domios mesh link <installation-id>",
        ),
        "revoked" => (
            "This machine was removed from your account.",
            "refresh the account machine list before retrying",
        ),
        "cancelled" | "conflict" => (
            "The account changed before the machine could be linked.",
            "refresh the account machine list, then retry domios mesh link <installation-id>",
        ),
        "rate-limited" => (
            "The account service is receiving too many requests.",
            "wait briefly, then retry domios mesh link <installation-id>",
        ),
        "mesh.machines" => (
            "Your account has reached its machine limit.",
            "review your Domios plan before linking another machine",
        ),
        "mesh-allowance" => (
            "Your account does not have an available mesh link.",
            "review your Domios plan, then retry domios mesh link <installation-id>",
        ),
        "expired" => (
            "The machine link authorization expired.",
            "refresh the account machine list, then retry domios mesh link <installation-id>",
        ),
        _ => {
            return Some((
                format!("Account Mesh refused the link ({reason})."),
                "refresh the account machine list, then retry domios mesh link <installation-id>"
                    .to_string(),
            ));
        }
    };
    Some((message.to_string(), hint.to_string()))
}

impl From<SocketCallError> for CliError {
    fn from(error: SocketCallError) -> Self {
        match error {
            SocketCallError::Transport(message) => Self::new(message),
            SocketCallError::Rpc(rpc) => Self {
                message: rpc_explanation(&rpc.message),
                code: Some(rpc.code),
                data: rpc.data,
            },
        }
    }
}

/// A rendered `HTTP <status>: <body>` (or `HTTP <status> — <body>`) failure line.
fn parse_http_line(text: &str) -> Option<(u16, &str)> {
    let rest = text.strip_prefix("HTTP ")?;
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let status = rest[..digits].parse().ok()?;
    let after = &rest[digits..];
    let body = after
        .find([':', '—'])
        .map(|at| {
            after[at..].trim_start_matches(|c: char| c == ':' || c == '—' || c.is_whitespace())
        })
        .unwrap_or("");
    Some((status, body))
}

/// The daemon's own message is the application-level explanation; an empty one
/// is reported as unexplained rather than dressed up as a cause we don't know.
pub(crate) fn rpc_explanation(message: &str) -> String {
    if message.trim().is_empty() {
        UNEXPLAINED_RPC_FAILURE.to_string()
    } else {
        message.to_string()
    }
}

pub fn handle_cli_result(json: bool, result: Result<serde_json::Value, CliError>) {
    match result {
        Ok(value) => {
            exit_if_cli_refusal(json, &value);
            if json {
                print_json(&value);
            } else {
                print_human(&value);
            }
        }
        Err(error) => exit_with_cli_error(json, &error),
    }
}

/// Exit after emitting a domain-level refusal returned as a successful RPC
/// result. Some daemon operations encode refusals in their result object rather
/// than using JSON-RPC's error member.
pub fn exit_if_cli_refusal(json: bool, value: &serde_json::Value) {
    if !is_cli_refusal(value) {
        return;
    }
    if json {
        println!("{}", crate::cmd::output::encode(value).unwrap());
    } else {
        let message = refusal_message(value).unwrap_or("command was refused");
        eprintln!("Error: {message}");
    }
    std::process::exit(CLI_ERROR_EXIT_CODE);
}

fn is_cli_refusal(value: &serde_json::Value) -> bool {
    if value.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
        return true;
    }
    [
        value,
        &value["error"],
        &value["data"],
        &value["error"]["data"],
    ]
    .iter()
    .any(|object| {
        ["kind", "type", "outcome"]
            .iter()
            .filter_map(|field| object.get(*field).and_then(serde_json::Value::as_str))
            .any(|tag| {
                tag == "refusal"
                    || tag == "refused"
                    || tag.ends_with("_refusal")
                    || tag.ends_with("_refused")
            })
    })
}

fn refusal_message(value: &serde_json::Value) -> Option<&str> {
    [&value["error"], value, &value["data"]]
        .iter()
        .find_map(|object| {
            ["message", "reason", "error"]
                .iter()
                .find_map(|field| object.get(*field).and_then(serde_json::Value::as_str))
        })
}

pub fn exit_with_cli_error(json: bool, error: &CliError) -> ! {
    write_cli_error(json, error, &mut std::io::stdout(), &mut std::io::stderr());
    std::process::exit(cli_error_exit_code(json));
}

fn cli_error_exit_code(_json: bool) -> i32 {
    CLI_ERROR_EXIT_CODE
}

/// Report a failure in the mode the caller asked for and leave with `exit_code`.
pub fn fail_with(exit_code: i32, error: &CliError) -> ! {
    write_cli_error(
        json_mode(),
        error,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    std::process::exit(exit_code);
}

pub fn fail(exit_code: i32, message: impl Into<String>) -> ! {
    fail_with(exit_code, &CliError::new(message))
}

/// A usage failure whose human form keeps its multi-line hint on stderr and
/// whose `--json` form carries the same hint inside the error object.
pub fn fail_with_hints(exit_code: i32, message: impl Into<String>, hints: &[&str]) -> ! {
    let json = json_mode();
    let hint = hints
        .iter()
        .map(|line| line.trim().trim_start_matches("hint: "))
        .collect::<Vec<_>>()
        .join(" ");
    let error = CliError {
        message: message.into(),
        code: None,
        data: Some(serde_json::json!({ "hint": hint })),
    };
    write_cli_error(json, &error, &mut std::io::stdout(), &mut std::io::stderr());
    if !json {
        for line in hints {
            eprintln!("{line}");
        }
    }
    std::process::exit(exit_code);
}

pub fn fail_http(exit_code: i32, status: u16, body: &str) -> ! {
    fail_with(exit_code, &CliError::http(status, body))
}

pub(crate) fn write_cli_error(
    json: bool,
    error: &CliError,
    out: &mut impl Write,
    err: &mut impl Write,
) {
    if json {
        let _ = writeln!(
            out,
            "{}",
            error_envelope_with_data(&error.message, error.code, error.data.as_ref())
        );
        return;
    }
    let _ = out.flush();
    let _ = writeln!(err, "Error: {}", error.message);
}

#[cfg(test)]
mod http_refusal_tests {
    use super::{write_cli_error, CliError};

    #[test]
    fn account_mesh_refusal_uses_a_repair_hint_without_echoing_http_json() {
        for (reason, expected, hint) in [
            (
                "unauthenticated",
                "account session could not be verified",
                "sign in again",
            ),
            (
                "verification-unavailable",
                "membership could not be verified",
                "sign in again",
            ),
            (
                "future-refusal",
                "Account Mesh refused the link (future-refusal).",
                "refresh the account machine list",
            ),
        ] {
            let body = format!(
                r#"{{"ok":false,"request_id":"request-1","failure":{{"reason":"{reason}"}}}}"#
            );
            let error = CliError::http(401, &body);
            let mut out = Vec::new();
            let mut err = Vec::new();
            write_cli_error(false, &error, &mut out, &mut err);
            let rendered = String::from_utf8(err).unwrap();

            assert!(rendered.contains(expected), "{rendered}");
            assert!(rendered.contains(hint), "{rendered}");
            assert!(!rendered.contains("request-1"), "{rendered}");
            assert!(!rendered.contains("HTTP 401"), "{rendered}");
            assert!(!rendered.contains("failure"), "{rendered}");
        }
    }
}

const ENTITY_ID_FIELDS: &[&str] = &[
    "id",
    "task_id",
    "plan_id",
    "spec_id",
    "section_id",
    "spec_section_id",
    "plan_section_id",
    "element_id",
    "milestone_id",
    "stride_id",
    "loop_id",
    "tab_id",
    "workspace_id",
    "service_id",
    "run_id",
    "attachment_id",
];

pub fn uniform_id_value(value: &serde_json::Value) -> serde_json::Value {
    let mut out = value.clone();
    rename_tab_json_keys(&mut out);
    add_uniform_id_alias(&mut out);
    out
}

/// The one key `tab list --json` wraps the list in, local and over `--mesh`.
pub const TAB_LIST_JSON_KEY: &str = "tabs";

#[cfg(test)]
pub fn tab_list_json_envelope(tabs: Vec<serde_json::Value>) -> serde_json::Value {
    tab_list_json_envelope_with_request(tabs, None)
}

pub fn tab_list_json_envelope_with_request(
    tabs: Vec<serde_json::Value>,
    requested_workspace: Option<&str>,
) -> serde_json::Value {
    let first = tabs.first();
    let kind = requested_workspace
        .map(|_| "workspace")
        .or_else(|| {
            first
                .and_then(|tab| tab.get("scope"))
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or("all_workspaces")
        .to_string();
    let workspace_id = requested_workspace.map(str::to_string).or_else(|| {
        first
            .and_then(|tab| tab.get("scope_workspace_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    });
    let workspace_name = first
        .and_then(|tab| tab.get("scope_workspace_name"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let other_workspace_tab_count = first
        .and_then(|tab| tab.get("other_workspace_tab_count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let mut grouped = std::collections::BTreeMap::<(String, String), Vec<serde_json::Value>>::new();
    for tab in &tabs {
        let id = tab
            .get("workspace_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let name = tab
            .get("workspace_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        grouped.entry((id, name)).or_default().push(tab.clone());
    }
    let workspaces = grouped
        .into_iter()
        .map(|((id, name), tabs)| {
            serde_json::json!({
                "id": id,
                "name": name,
                "tabs": tabs,
            })
        })
        .collect::<Vec<_>>();
    let mut envelope = serde_json::Map::new();
    envelope.insert(
        TAB_LIST_JSON_KEY.to_string(),
        serde_json::Value::Array(tabs),
    );
    envelope.insert(
        "scope".into(),
        serde_json::json!({
            "kind": kind,
            "workspace_id": workspace_id,
            "workspace_name": workspace_name,
            "other_workspace_tab_count": other_workspace_tab_count,
        }),
    );
    envelope.insert("workspaces".into(), serde_json::Value::Array(workspaces));
    serde_json::Value::Object(envelope)
}

pub fn print_json(value: &serde_json::Value) {
    println!(
        "{}",
        crate::cmd::output::encode(value).unwrap()
    );
}

fn add_uniform_id_alias(value: &mut serde_json::Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    if obj.get("id").and_then(|v| v.as_str()).is_some() {
        return;
    }
    let Some(id) = ENTITY_ID_FIELDS.iter().find_map(|field| {
        obj.get(*field)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }) else {
        return;
    };
    obj.insert("id".into(), serde_json::Value::String(id));
}

fn rename_tab_json_keys(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                rename_tab_json_keys(item);
            }
        }
        serde_json::Value::Object(object) => {
            for item in object.values_mut() {
                rename_tab_json_keys(item);
            }
            let keys: Vec<String> = object.keys().cloned().collect();
            for key in keys {
                let tab_key = tab_json_key(&key);
                if tab_key != key {
                    if let Some(value) = object.remove(&key) {
                        object.entry(tab_key).or_insert(value);
                    }
                }
            }
        }
        _ => {}
    }
}

fn tab_json_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let chars: Vec<char> = key.chars().collect();
    let mut segment_start = 0;
    for index in 1..=chars.len() {
        let boundary = index == chars.len()
            || chars[index] == '_'
            || chars[index] == '-'
            || chars[index].is_ascii_uppercase()
                && (chars[index - 1].is_ascii_lowercase()
                    || chars[index - 1].is_ascii_uppercase()
                        && chars
                            .get(index + 1)
                            .is_some_and(|next| next.is_ascii_lowercase()));
        if !boundary {
            continue;
        }
        out.push_str(&tab_json_segment(&chars[segment_start..index]));
        if index < chars.len() && (chars[index] == '_' || chars[index] == '-') {
            out.push(chars[index]);
            segment_start = index + 1;
        } else {
            segment_start = index;
        }
    }
    out
}

fn tab_json_segment(segment: &[char]) -> String {
    let source: String = segment.iter().collect();
    match source.as_str() {
        "pane" => "tab".into(),
        "panes" => "tabs".into(),
        "Pane" => "Tab".into(),
        "Panes" => "Tabs".into(),
        "PANE" => "TAB".into(),
        "PANES" => "TABS".into(),
        _ => source,
    }
}

/// String failures are the same failures; they reach the same `--json` sink,
/// only without a daemon code to carry.
pub fn handle_result(json: bool, result: Result<serde_json::Value, String>) {
    handle_cli_result(json, result.map_err(CliError::new));
}

pub fn print_human(value: &serde_json::Value) {
    if !crate::cmd::output::verbose() && (value.is_array() || crate::cmd::output::is_collection() && crate::cmd::output::has_collection(value)) {
        print!("{}", crate::cmd::output::compact_list(value, None));
        return;
    }
    match value {
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                println!("(empty)");
                return;
            }
            for item in arr {
                print_object(item);
                println!();
            }
        }
        serde_json::Value::Object(_) => {
            print_object(value);
        }
        serde_json::Value::String(s) => println!("{}", s),
        serde_json::Value::Null => println!("OK"),
        other => println!("{}", other),
    }
}

pub fn print_object(value: &serde_json::Value) {
    if crate::cmd::output::is_collection() && !crate::cmd::output::verbose() && crate::cmd::output::has_collection(value) {
        print!("{}", crate::cmd::output::compact_list(value, None));
        return;
    }
    let mut value = value.clone();
    rename_tab_json_keys(&mut value);
    if let Some(obj) = value.as_object() {
        let max_key_len = obj.keys().map(|k| k.len()).max().unwrap_or(0);
        for (key, val) in obj {
            let display_val = match val {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(a) => {
                    let items: Vec<String> = a
                        .iter()
                        .map(|v| match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .collect();
                    items.join(", ")
                }
                serde_json::Value::Null => "(none)".to_string(),
                other => other.to_string(),
            };
            println!("  {:width$}  {}", key, display_val, width = max_key_len);
        }
    }
}

pub fn print_history_timeline(rows: &[serde_json::Value]) {
    if rows.is_empty() {
        println!("(no history)");
        return;
    }
    for row in rows {
        let changed_at = row
            .get("changed_at")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        // Drop sub-second precision + tz for compact display.
        let ts = changed_at
            .split('.')
            .next()
            .unwrap_or(changed_at)
            .replace('T', " ");
        let changed_by = row
            .get("changed_by")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let change_type = row
            .get("change_type")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let row_id = row.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        println!("* [{}] {}  {:<15} {}", row_id, ts, changed_by, change_type);
        let diff_line =
            |label: &str, old: Option<&serde_json::Value>, new: Option<&serde_json::Value>| {
                let has_old = old.map(|v| !v.is_null()).unwrap_or(false);
                let has_new = new.map(|v| !v.is_null()).unwrap_or(false);
                if !has_old && !has_new {
                    return None;
                }
                let fmt = |v: Option<&serde_json::Value>| -> String {
                    match v {
                        None | Some(serde_json::Value::Null) => "-".into(),
                        Some(serde_json::Value::String(s)) => {
                            let truncated: String = s.chars().take(60).collect();
                            if s.chars().count() > 60 {
                                format!("{}…", truncated)
                            } else {
                                truncated
                            }
                        }
                        Some(other) => other.to_string(),
                    }
                };
                if has_old && has_new {
                    Some(format!(
                        "    {:<11} {} -> {}",
                        format!("{}:", label),
                        fmt(old),
                        fmt(new)
                    ))
                } else if has_new {
                    Some(format!("    {:<11} {}", format!("{}:", label), fmt(new)))
                } else {
                    Some(format!(
                        "    {:<11} (deleted: {})",
                        format!("{}:", label),
                        fmt(old)
                    ))
                }
            };
        for (label, old_k, new_k) in [
            ("title", "old_title", "new_title"),
            ("kind", "old_kind", "new_kind"),
            ("importance", "old_importance", "new_importance"),
            ("body", "old_body", "new_body"),
        ] {
            if let Some(line) = diff_line(label, row.get(old_k), row.get(new_k)) {
                println!("{}", line);
            }
        }
        if let Some(reason) = row.get("reason").and_then(|v| v.as_str()) {
            println!("    reason:     {}", reason);
        }
    }
}

pub fn print_taxonomy(value: &serde_json::Value) {
    let Some(obj) = value.as_object() else {
        print_human(value);
        return;
    };
    let mut areas: Vec<&String> = obj.keys().collect();
    // Alphabetical, with "(none)" pushed to the bottom.
    areas.sort_by(|a, b| match (a.as_str(), b.as_str()) {
        ("(none)", "(none)") => std::cmp::Ordering::Equal,
        ("(none)", _) => std::cmp::Ordering::Greater,
        (_, "(none)") => std::cmp::Ordering::Less,
        (x, y) => x.cmp(y),
    });
    if areas.is_empty() {
        println!("(empty)");
        return;
    }
    for area in areas {
        println!("{}", area);
        if let Some(kinds) = obj.get(area).and_then(|v| v.as_object()) {
            let mut kind_keys: Vec<&String> = kinds.keys().collect();
            kind_keys.sort();
            for k in kind_keys {
                let n = kinds.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
                println!("  {:<13} {}", k, n);
            }
        }
    }
}

#[cfg(test)]
mod tests;

pub fn finish_app_quit(output: codeterm_types::app_quit::QuitOutput, json: bool) -> ! {
    use codeterm_types::app_quit::QuitOutput;
    let confirmed = matches!(output, QuitOutput::Confirmed { .. });
    if json {
        println!(
            "{}",
            crate::cmd::output::encode(&output).expect("Quit output serializes")
        );
    } else {
        match &output {
            QuitOutput::Confirmed { .. } => {
                println!("Application and server exited after successful cleanup.")
            }
            QuitOutput::Refused { reason } => {
                println!("Quit refused: {reason:?}. No replacement is authorized.")
            }
            QuitOutput::Uncertain { reason } => {
                println!("Quit uncertain: {reason:?}. No replacement is authorized.")
            }
        }
    }
    std::process::exit(if confirmed { 0 } else { 1 });
}
