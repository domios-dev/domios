use super::{resolve_report_body, ReportBodyInput};
use crate::cmd::resolved_text::resolved_text_echo;

const BODY: &str = "STATUS: in_review\nCHANGES: cli/src/cmd/report_body.rs\nTESTS RUN: deferred\nGATES REQUESTED: none\nFOLLOWUP: orchestrator closes";

fn echoed_body(echo: &str) -> String {
    let encoded = echo
        .strip_prefix("CODETERM_RESOLVED_TEXT=")
        .unwrap_or_else(|| panic!("report echo must use the shared send prefix: {echo}"));
    serde_json::from_str(encoded).expect("report echo payload is JSON")
}

#[test]
fn report_echo_carries_the_body_for_every_delivery_mechanism() {
    let mechanisms: Vec<(&str, ReportBodyInput<'_>)> = vec![
        (
            "positional",
            ReportBodyInput {
                message: Some(BODY),
                ..Default::default()
            },
        ),
        (
            "--text",
            ReportBodyInput {
                text: Some(BODY),
                ..Default::default()
            },
        ),
        (
            "--from-pad",
            ReportBodyInput {
                pad: Some(BODY),
                ..Default::default()
            },
        ),
        (
            "--doc",
            ReportBodyInput {
                doc: Some(("report.md", BODY)),
                ..Default::default()
            },
        ),
    ];
    for (mechanism, input) in mechanisms {
        let resolved = resolve_report_body(input);
        let echo = resolved_text_echo(&resolved);
        assert_eq!(
            echo.lines().count(),
            1,
            "{mechanism} echo must stay one line"
        );
        let carried = echoed_body(&echo);
        for field in [
            "STATUS:",
            "CHANGES:",
            "TESTS RUN:",
            "GATES REQUESTED:",
            "FOLLOWUP:",
        ] {
            assert!(
                carried.contains(field),
                "{mechanism} echo must carry {field}: {carried}"
            );
        }
    }
}

#[test]
fn report_echo_redacts_secrets_before_leaving_the_process() {
    let body = format!("{BODY}\nNOTES: sk-live-super-secret-token and API_TOKEN=hunter2");
    for input in [
        ReportBodyInput {
            message: Some(&body),
            ..Default::default()
        },
        ReportBodyInput {
            pad: Some(&body),
            ..Default::default()
        },
    ] {
        let echo = resolved_text_echo(&resolve_report_body(input));
        assert!(echo.contains("***REDACTED***"));
        assert!(!echo.contains("sk-live-super-secret-token"));
        assert!(!echo.contains("hunter2"));
    }
}

#[test]
fn report_body_precedence_is_pad_then_text_then_positional() {
    assert_eq!(
        resolve_report_body(ReportBodyInput {
            pad: Some("pad"),
            text: Some("text"),
            message: Some("positional"),
            doc: None,
        }),
        "pad"
    );
    assert_eq!(
        resolve_report_body(ReportBodyInput {
            text: Some("text"),
            message: Some("positional"),
            ..Default::default()
        }),
        "text"
    );
    assert_eq!(
        resolve_report_body(ReportBodyInput {
            message: Some("positional"),
            ..Default::default()
        }),
        "positional"
    );
    assert_eq!(resolve_report_body(ReportBodyInput::default()), "");
}

#[test]
fn report_body_appends_doc_under_its_path_header() {
    assert_eq!(
        resolve_report_body(ReportBodyInput {
            text: Some("STATUS: in_review"),
            doc: Some(("notes.md", "detail")),
            ..Default::default()
        }),
        "STATUS: in_review\n\ndoc: notes.md\n\ndetail"
    );
    assert_eq!(
        resolve_report_body(ReportBodyInput {
            doc: Some(("notes.md", "detail")),
            ..Default::default()
        }),
        "doc: notes.md\n\ndetail",
        "a doc-only report must not lead with a blank separator"
    );
}
