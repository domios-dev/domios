use std::io::Write;

use crate::client::SocketCallError;
use crate::cmd::plan_output::error_envelope;

/// Exit status of every failed CLI command, with and without `--json`.
pub const CLI_ERROR_EXIT_CODE: i32 = 1;

const UNEXPLAINED_RPC_FAILURE: &str = "the daemon rejected this command without an explanation";

/// A CLI failure split into the human-facing explanation and the machine-facing
/// JSON-RPC code, so the code can reach `--json` without reaching the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    pub message: String,
    pub code: Option<i32>,
}

impl CliError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
        }
    }
}

impl From<SocketCallError> for CliError {
    fn from(error: SocketCallError) -> Self {
        match error {
            SocketCallError::Transport(message) => Self::new(message),
            SocketCallError::Rpc(rpc) => Self {
                message: rpc_explanation(&rpc.message),
                code: Some(rpc.code),
            },
        }
    }
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
            if json {
                print_json(&value);
            } else {
                print_human(&value);
            }
        }
        Err(error) => exit_with_cli_error(json, &error),
    }
}

pub fn exit_with_cli_error(json: bool, error: &CliError) -> ! {
    write_cli_error(
        json,
        error,
        &mut std::io::stdout(),
        &mut std::io::stderr().lock(),
    );
    std::process::exit(cli_error_exit_code(json));
}

fn cli_error_exit_code(_json: bool) -> i32 {
    CLI_ERROR_EXIT_CODE
}

pub(crate) fn write_cli_error(
    json: bool,
    error: &CliError,
    out: &mut impl Write,
    err: &mut impl Write,
) {
    let _ = out.flush();
    let rendered = if json {
        error_envelope(&error.message, error.code).to_string()
    } else {
        format!("Error: {}", error.message)
    };
    let _ = writeln!(err, "{rendered}");
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

pub fn tab_list_json_envelope(tabs: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({ "tabs": tabs })
}

pub fn print_json(value: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(&uniform_id_value(value)).unwrap()
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

pub fn handle_result(json: bool, result: Result<serde_json::Value, String>) {
    match result {
        Ok(value) => {
            if json {
                print_json(&value);
            } else {
                print_human(&value);
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

pub fn print_human(value: &serde_json::Value) {
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
mod tests {
    use super::*;
    use crate::client::RpcError;
    use serde_json::json;

    /// Every JSON-RPC code the daemon uses starts with this; asserting its
    /// absence is structural, unlike pinning a message phrase.
    const RPC_CODE_MARKER: &str = "-32";

    fn tab_not_found() -> SocketCallError {
        SocketCallError::Rpc(RpcError {
            code: -32603,
            message: "one or more requested Tabs were not found".into(),
            data: None,
        })
    }

    fn rendered(json: bool, error: &CliError) -> (String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        write_cli_error(json, error, &mut out, &mut err);
        (
            String::from_utf8(out).expect("stdout is utf8"),
            String::from_utf8(err).expect("stderr is utf8"),
        )
    }

    #[test]
    fn json_error_envelope_goes_to_stderr_and_leaves_stdout_empty() {
        let (out, err) = rendered(true, &CliError::from(tab_not_found()));

        assert!(out.is_empty(), "stdout must stay parseable: {out}");
        let envelope: serde_json::Value =
            serde_json::from_str(&err).expect("stderr carries exactly one JSON envelope");
        assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
        assert_eq!(envelope["kind"], "error");
        assert_eq!(envelope["code"], -32603);
    }

    #[test]
    fn human_error_goes_to_stderr_without_the_raw_rpc_code() {
        let (out, err) = rendered(false, &CliError::from(tab_not_found()));

        assert!(out.is_empty(), "stdout must stay parseable: {out}");
        assert!(!err.contains(RPC_CODE_MARKER), "{err}");
        assert!(err.contains("one or more requested Tabs were not found"), "{err}");
    }

    #[test]
    fn error_exit_code_is_non_zero_and_identical_in_both_modes() {
        assert_eq!(cli_error_exit_code(true), cli_error_exit_code(false));
        assert_ne!(cli_error_exit_code(true), 0);
        assert_eq!(cli_error_exit_code(false), CLI_ERROR_EXIT_CODE);
    }

    #[test]
    fn rpc_failure_keeps_the_code_programmatically_and_out_of_the_message() {
        let error = CliError::from(tab_not_found());

        assert_eq!(error.code, Some(-32603));
        assert!(!error.message.contains(RPC_CODE_MARKER), "{}", error.message);
    }

    #[test]
    fn empty_rpc_message_is_reported_unexplained_instead_of_invented() {
        let error = CliError::from(SocketCallError::Rpc(RpcError {
            code: -32000,
            message: "   ".into(),
            data: None,
        }));

        assert_eq!(error.message, UNEXPLAINED_RPC_FAILURE);
        assert_eq!(error.code, Some(-32000));
    }

    #[test]
    fn transport_failure_has_no_code_and_keeps_its_own_text() {
        let error = CliError::from(SocketCallError::Transport("socket is gone".into()));

        assert_eq!(error.code, None);
        assert_eq!(error.message, "socket is gone");
    }

    #[test]
    fn locally_validated_error_still_emits_the_envelope_with_a_null_code() {
        let (out, err) = rendered(true, &CliError::new("invalid remote tab id"));

        assert!(out.is_empty(), "stdout must stay parseable: {out}");
        let envelope: serde_json::Value = serde_json::from_str(&err).expect("one JSON envelope");
        assert_eq!(envelope["ok"], serde_json::Value::Bool(false));
        assert_eq!(envelope["code"], serde_json::Value::Null);
        assert_eq!(envelope["error"], "invalid remote tab id");
    }

    #[test]
    fn tab_list_json_uses_object_envelope_matching_plan_task_list() {
        let enveloped = tab_list_json_envelope(vec![json!({
            "id": "tab_1",
            "title": "one",
            "session_binding_nonce": "nonce-recorded",
            "reported_session_binding_nonce": "nonce-reported",
        })]);
        assert!(
            enveloped.is_object(),
            "tab list --json must be an object envelope, not a bare array"
        );
        let inner = enveloped
            .get("tabs")
            .expect("envelope carries a tabs field");
        assert!(inner.is_array(), "tabs field holds the list");
        assert_eq!(inner.as_array().unwrap().len(), 1);
        assert_eq!(inner[0]["session_binding_nonce"], "nonce-recorded");
        assert_eq!(inner[0]["reported_session_binding_nonce"], "nonce-reported");
    }

    #[test]
    fn uniform_id_alias_preserves_existing_id() {
        let out = uniform_id_value(&json!({ "id": "canonical", "task_id": "task_1" }));
        assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("canonical"));
    }

    #[test]
    fn uniform_id_alias_adds_top_level_id_for_plan_create() {
        let out = uniform_id_value(&json!({ "plan_id": "plan_1", "title": "Plan" }));
        assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("plan_1"));
        assert_eq!(out.get("plan_id").and_then(|v| v.as_str()), Some("plan_1"));
    }

    #[test]
    fn uniform_id_alias_adds_top_level_id_for_task_add() {
        let out = uniform_id_value(&json!({ "plan_id": "plan_1", "task_id": "task_1" }));
        assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("task_1"));
    }

    #[test]
    fn uniform_id_alias_adds_top_level_id_for_agent_spawn() {
        let out = uniform_id_value(&json!({ "tab_id": "tab_1", "agent": "codex" }));
        assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("tab_1"));
    }

    #[test]
    fn json_output_uses_tab_field_names_recursively() {
        let out = uniform_id_value(&json!({
            "tab_id": "tab_1",
            "parent_tab_id": "tab_0",
            "tabs": [{ "createdByTab": "tab_2" }]
        }));
        assert_eq!(out.get("tab_id").and_then(|v| v.as_str()), Some("tab_1"));
        assert_eq!(
            out.get("parent_tab_id").and_then(|v| v.as_str()),
            Some("tab_0")
        );
        assert_eq!(
            out.pointer("/tabs/0/createdByTab").and_then(|v| v.as_str()),
            Some("tab_2")
        );
    }

    #[test]
    fn json_output_preserves_string_values_named_tab() {
        let input = json!({
            "tab_id": "tab_1",
            "title": "tab",
            "message": "tabs",
            "nested": [{ "body": "tab", "label": "tabs" }]
        });
        let out = uniform_id_value(&input);
        assert_eq!(out["title"], input["title"]);
        assert_eq!(out["message"], input["message"]);
        assert_eq!(out["nested"][0]["body"], input["nested"][0]["body"]);
        assert_eq!(out["nested"][0]["label"], input["nested"][0]["label"]);
    }

    #[test]
    fn json_output_keeps_canonical_tab_key_when_legacy_key_collides() {
        let out = uniform_id_value(&json!({
            "pane_id": "legacy",
            "tab_id": "canonical",
            "createdByPane": "legacy-creator",
            "createdByTab": "canonical-creator"
        }));
        assert!(out.get("pane_id").is_none());
        assert_eq!(out["tab_id"], "canonical");
        assert_eq!(out["id"], "canonical");
        assert!(out.get("createdByPane").is_none());
        assert_eq!(out["createdByTab"], "canonical-creator");
    }

    #[test]
    fn human_output_uses_tab_field_names_without_changing_values() {
        let mut value = serde_json::json!({
            "pane_type": "pane",
            "parentPaneId": "parent-pane"
        });

        rename_tab_json_keys(&mut value);

        assert_eq!(value["tab_type"], "pane");
        assert_eq!(value["parentTabId"], "parent-pane");
        assert!(value.get("pane_type").is_none());
        assert!(value.get("parentPaneId").is_none());
    }
}
