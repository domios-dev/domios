use crate::client::SocketClient;
use crate::cmd::report_guard::ReportRefusal;

pub(super) fn refusal_next_commands() -> Vec<&'static str> {
    crate::cmd::report_guard::REPORT_NEXT_COMMANDS.to_vec()
}

pub(super) fn refusal_diagnostic_request(
    tab_id: &str,
    refusal: &ReportRefusal,
    original: &str,
) -> (&'static str, serde_json::Value) {
    (
        "agent.diagnostic",
        serde_json::json!({
            "tab_id": tab_id,
            "kind": "report_refused",
            "message": format!(
                "REPORT NOT SENT\n{}\n\nOriginal readiness report that was refused:\n{}",
                refusal.message(), original
            ),
        }),
    )
}

pub(super) fn format_report_refusal_text(
    refusal: &ReportRefusal,
    sent: bool,
    target_tab_id: Option<&str>,
    error: Option<&str>,
) -> String {
    let mut lines = vec!["REPORT NOT SENT".to_string(), refusal.message()];
    match (sent, target_tab_id, error) {
        (true, Some(id), _) => {
            lines.push(format!("Refused report diagnostic sent to parent: {id}"))
        }
        (true, None, _) => lines.push("Refused report diagnostic sent to parent.".to_string()),
        (false, _, Some(err)) => {
            lines.push(format!("Refused report diagnostic could not be sent: {err}"))
        }
        (false, _, None) => lines.push("Refused report diagnostic could not be sent.".to_string()),
    }
    lines.push("REPORT NOT SENT".to_string());
    format!("{}\n", lines.join("\n"))
}

pub(super) fn report_refusal_json(
    refusal: &ReportRefusal,
    diagnostic_sent: bool,
    target_tab_id: Option<String>,
    diagnostic_error: Option<String>,
) -> serde_json::Value {
    serde_json::json!({
        "verdict": "REPORT NOT SENT",
        "reason": refusal.reason.clone(),
        "dirty_files": refusal.dirty_files.clone(),
        "next_commands": refusal_next_commands(),
        "refused_report_diagnostic_sent": diagnostic_sent,
        "diagnostic_target_tab_id": target_tab_id,
        "diagnostic_error": diagnostic_error,
    })
}

pub(super) fn handle_report_refusal(
    client: &mut SocketClient,
    json: bool,
    body: serde_json::Value,
    refusal: ReportRefusal,
) -> ! {
    let original = body
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let tab_id = body
        .get("tab_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let (method, diagnostic_body) = refusal_diagnostic_request(tab_id, &refusal, &original);
    let diagnostic = client.call_detailed(method, Some(diagnostic_body));
    let (sent, target_tab_id, error) = match diagnostic {
        Ok(value) => (
            true,
            value
                .get("target_tab_id")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            None,
        ),
        Err(err) => (false, None, Some(err.to_string())),
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report_refusal_json(
                &refusal,
                sent,
                target_tab_id,
                error,
            ))
            .unwrap()
        );
    } else {
        eprint!(
            "{}",
            format_report_refusal_text(
                &refusal,
                sent,
                target_tab_id.as_deref(),
                error.as_deref(),
            )
        );
    }
    std::process::exit(1);
}

