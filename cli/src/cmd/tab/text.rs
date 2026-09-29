use crate::cmd::print::{fail, fail_http, print_json};

/// Screen text as the command's success value: `--json` wraps it in one object,
/// and a missing or unreadable tab is a failure, never an empty success.
pub(super) fn print_tab_text(json: bool, formatted: bool, tab_id: &str, text: &str) {
    if json {
        print_json(&serde_json::json!({
            "ok": true,
            "tab_id": tab_id,
            "formatted": formatted,
            "text": text,
        }));
    } else if formatted {
        print!("{text}");
    } else {
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            println!("{line}");
        }
    }
}

pub(super) fn print_local_tab_text(
    json: bool,
    formatted: bool,
    tab_id: &str,
    status: u16,
    body: &str,
) {
    if !(200..300).contains(&status) {
        fail_http(1, status, body);
    }
    let value: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let resolved = value
        .get("tab_id")
        .and_then(|id| id.as_str())
        .unwrap_or(tab_id);
    match value.get("text").and_then(|text| text.as_str()) {
        Some(text) => print_tab_text(json, formatted, resolved, text),
        None => fail(1, format!("tab {tab_id} returned no screen text")),
    }
}

pub(super) fn print_exec_output(json: bool, output: &str) {
    if json {
        print_json(&serde_json::json!({ "ok": true, "output": output }));
    } else {
        print!("{output}");
    }
}

pub(super) fn print_local_exec(json: bool, status: u16, body: &str) {
    if !(200..300).contains(&status) {
        fail_http(1, status, body);
    }
    let value: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    match value.get("output").and_then(|output| output.as_str()) {
        Some(output) => print_exec_output(json, output),
        None => fail(
            1,
            value
                .get("error")
                .and_then(|error| error.as_str())
                .unwrap_or("exec returned no output")
                .to_string(),
        ),
    }
}
