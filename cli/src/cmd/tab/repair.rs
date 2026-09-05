use crate::client::SocketClient;
use crate::cmd::print::print_json;
use crate::{build_local_api, read_api_config, resolve_tab};

use super::remote_target::{classify_remote_tab_id, RemoteTabTarget};

const LOCAL_ONLY: &str = "tab repair is caller-local and cannot target a mesh peer";

pub(super) fn handle_tab_repair(
    client: &mut SocketClient,
    json: bool,
    provider: Option<String>,
    session: Option<String>,
    from_evidence: bool,
    from_journal: bool,
    tab: Option<String>,
) {
    if tab
        .as_deref()
        .is_some_and(|value| !matches!(classify_remote_tab_id(value), RemoteTabTarget::Local))
    {
        fail(
            json,
            serde_json::json!({
                "ok": false,
                "error": {"code": "tab_repair_local_only", "message": LOCAL_ONLY},
            }),
        );
    }
    let tab_id = resolve_tab(client, tab);
    let body = repair_request(provider, session, from_evidence, from_journal);
    let (api_url, _) = read_api_config();
    let api = build_local_api().unwrap_or_else(|error| {
        fail(
            json,
            serde_json::json!({
                "ok": false,
                "error": {"code": "tab_repair_transport_failed", "message": error.to_string()},
            }),
        )
    });
    match api
        .post(format!("{api_url}/api/tabs/{tab_id}/repair"))
        .json(&body)
        .send()
    {
        Ok(response) => {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            let value = serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|_| {
                serde_json::json!({
                    "ok": false,
                    "error": {
                        "code": "tab_repair_invalid_response",
                        "message": if text.is_empty() { format!("HTTP {status}") } else { text },
                    },
                })
            });
            if status.is_success() {
                print_repair_result(json, &value);
            } else {
                fail(json, value);
            }
        }
        Err(error) => fail(
            json,
            serde_json::json!({
                "ok": false,
                "error": {"code": "tab_repair_transport_failed", "message": error.to_string()},
            }),
        ),
    }
}

fn repair_request(
    provider: Option<String>,
    session: Option<String>,
    from_evidence: bool,
    from_journal: bool,
) -> serde_json::Value {
    let default_evidence = !from_journal && provider.is_none() && session.is_none();
    serde_json::json!({
        "provider": provider,
        "session_id": session,
        "from_evidence": from_evidence || default_evidence,
        "from_journal": from_journal,
    })
}

fn print_repair_result(json: bool, value: &serde_json::Value) {
    if json {
        print_json(value);
        return;
    }
    let tab = value
        .get("tab_id")
        .and_then(|item| item.as_str())
        .unwrap_or("-");
    let source = value
        .get("source")
        .and_then(|item| item.as_str())
        .unwrap_or("verified evidence")
        .replace('_', " ");
    println!("Tab {tab}: repaired from {source}");
    print_field("provider", value.get("provider"));
    print_field("session", value.get("session"));
    print_field("parser lock", value.get("parser_lock"));
    if let Some(evidence) = value.get("evidence") {
        let session = evidence
            .get("session_id")
            .and_then(|item| item.as_str())
            .unwrap_or("-");
        let provider = evidence
            .get("provider")
            .and_then(|item| item.as_str())
            .unwrap_or("-");
        let record = evidence
            .get("session_record_verified")
            .and_then(|item| item.as_bool())
            .map(|verified| if verified { "verified" } else { "not verified" })
            .unwrap_or("unknown");
        let nonce = evidence
            .get("nonce_verified")
            .and_then(|item| item.as_bool())
            .map(|verified| if verified { "verified" } else { "not verified" })
            .unwrap_or("unknown");
        println!(
            "evidence: session {session}, provider {provider}, provider record {record}, binding nonce {nonce}"
        );
    }
    if let Some(restored) = value.pointer("/journal/restored_from") {
        let seq = restored
            .get("seq")
            .and_then(|item| item.as_u64())
            .unwrap_or(0);
        let operation = restored
            .get("operation_id")
            .and_then(|item| item.as_str())
            .unwrap_or("-");
        let recorded = restored
            .get("recorded_at")
            .and_then(|item| item.as_str())
            .map(|value| format!(", recorded {value}"))
            .unwrap_or_default();
        println!("journal: restored from seq {seq}, operation {operation}{recorded}");
    }
    if let Some(skipped) = value
        .pointer("/journal/skipped")
        .and_then(|item| item.as_array())
    {
        for record in skipped {
            let seq = record
                .get("seq")
                .and_then(|item| item.as_u64())
                .unwrap_or(0);
            let operation = record
                .get("operation_id")
                .and_then(|item| item.as_str())
                .unwrap_or("-");
            let reason = record
                .get("reason")
                .and_then(|item| item.as_str())
                .unwrap_or("not eligible");
            let recorded = record
                .get("recorded_at")
                .and_then(|item| item.as_str())
                .map(|value| format!(", recorded {value}"))
                .unwrap_or_default();
            println!("journal: skipped seq {seq}, operation {operation}{recorded}: {reason}");
        }
    }
}

fn print_field(label: &str, field: Option<&serde_json::Value>) {
    let Some(field) = field else {
        println!("{label}: could not verify");
        return;
    };
    let status = field
        .get("status")
        .and_then(|item| item.as_str())
        .unwrap_or("could_not_verify");
    let previous = field
        .get("previous")
        .and_then(value_label)
        .unwrap_or_else(|| "unset".into());
    let current = field
        .get("current")
        .and_then(value_label)
        .unwrap_or_else(|| "unset".into());
    match status {
        "kept" => println!("{label}: kept {current}"),
        "corrected" => println!("{label}: corrected from {previous} to {current}"),
        _ => println!("{label}: could not verify"),
    }
}

fn value_label(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_bool().map(|value| value.to_string()))
}

fn fail(json: bool, value: serde_json::Value) -> ! {
    if json {
        print_json(&value);
    } else {
        let code = value
            .pointer("/error/code")
            .and_then(|item| item.as_str())
            .unwrap_or("tab_repair_failed");
        let message = value
            .pointer("/error/message")
            .and_then(|item| item.as_str())
            .unwrap_or("the daemon refused tab identity repair");
        eprintln!("Repair failed [{code}]: {message}");
        for (label, key) in [
            ("provider", "provider"),
            ("session", "session"),
            ("parser lock", "parser_lock"),
        ] {
            if value
                .get(key)
                .and_then(|field| field.get("status"))
                .and_then(|status| status.as_str())
                == Some("could_not_verify")
            {
                eprintln!("{label}: could not verify");
            }
        }
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_mode_flag_defaults_to_live_evidence() {
        let request = repair_request(None, None, false, false);

        assert_eq!(request["from_evidence"], true);
        assert_eq!(request["from_journal"], false);
        assert!(request["provider"].is_null());
        assert!(request["session_id"].is_null());
    }

    #[test]
    fn explicit_modes_are_preserved_on_the_wire() {
        let provider = repair_request(Some("codex".into()), None, false, false);
        let session = repair_request(None, Some("session-1".into()), false, false);
        let journal = repair_request(None, None, false, true);

        assert_eq!(provider["provider"], "codex");
        assert_eq!(provider["from_evidence"], false);
        assert_eq!(session["session_id"], "session-1");
        assert_eq!(journal["from_journal"], true);
    }
}
