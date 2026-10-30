use std::cell::Cell;

use reqwest::StatusCode;
use serde_json::json;

use super::*;

/// Mirrors the message `LocalApiResponse::error_for_status` builds, which is
/// the only string `classify` can put into `ThemeOutcome::Failed`.
fn http_failure(status: StatusCode) -> ThemeOutcome {
    ThemeOutcome::Failed(format!("HTTP status {status}: {{\"error\":\"nope\"}}"))
}

fn never_applies() -> ThemeOutcome {
    unreachable!("the apply leg must not run")
}

fn failing_leaves(status: StatusCode) -> Vec<(&'static str, LeafOutput, LeafOutput)> {
    vec![
        (
            "list",
            list_output(false, http_failure(status)),
            list_output(true, http_failure(status)),
        ),
        (
            "apply",
            apply_output(false, "dark", http_failure(status)),
            apply_output(true, "dark", http_failure(status)),
        ),
        (
            "delete",
            delete_output(false, "dark", http_failure(status)),
            delete_output(true, "dark", http_failure(status)),
        ),
        (
            "save",
            save_output(false, "dark", false, http_failure(status), never_applies),
            save_output(true, "dark", false, http_failure(status), never_applies),
        ),
    ]
}

#[test]
fn every_http_leaf_fails_with_no_success_line_in_both_modes() {
    for status in [StatusCode::NOT_FOUND, StatusCode::INTERNAL_SERVER_ERROR] {
        for (leaf, human, json) in failing_leaves(status) {
            for (mode, output) in [("human", human), ("json", json)] {
                match output {
                    LeafOutput::Failure { earned, .. } => assert!(
                        earned.is_empty(),
                        "{leaf}/{mode} printed a success line on {status}: {earned:?}"
                    ),
                    other => {
                        panic!("{leaf}/{mode} did not fail on {status}: {other:?}")
                    }
                }
            }
        }
    }
}

#[test]
fn the_failure_verdict_is_identical_with_and_without_json() {
    for status in [StatusCode::BAD_REQUEST, StatusCode::BAD_GATEWAY] {
        for (leaf, human, json) in failing_leaves(status) {
            assert_eq!(
                human, json,
                "{leaf} classified {status} differently between modes"
            );
        }
    }
}

#[test]
fn the_failure_message_survives_into_the_leaf_output() {
    let status = StatusCode::NOT_FOUND;
    let ThemeOutcome::Failed(expected) = http_failure(status) else {
        panic!("fixture must be a failure");
    };
    let LeafOutput::Failure { message, .. } = apply_output(true, "dark", http_failure(status))
    else {
        panic!("a {status} must fail");
    };
    assert_eq!(message, expected);
}

#[test]
fn a_2xx_body_shaped_like_an_error_is_still_a_success() {
    let output = list_output(
        false,
        ThemeOutcome::Ok(json!({"ok": false, "error": "boom"})),
    );
    assert_eq!(output, LeafOutput::Human(Vec::new()));
}

#[test]
fn list_renders_names_in_human_mode_and_the_envelope_under_json() {
    let body = json!({"themes": ["dark", "light"]});
    assert_eq!(
        list_output(false, ThemeOutcome::Ok(body.clone())),
        LeafOutput::Human(vec!["dark".to_string(), "light".to_string()])
    );
    let LeafOutput::Json(envelope) = list_output(true, ThemeOutcome::Ok(body.clone())) else {
        panic!("--json must emit a document");
    };

    assert_eq!(envelope, body);
}

#[test]
fn apply_and_delete_carry_the_theme_id_through_the_envelope() {
    let body = json!({"applied": "dark"});
    for (leaf, human, json_output) in [
        (
            "Theme applied: dark",
            apply_output(false, "dark", ThemeOutcome::Ok(body.clone())),
            apply_output(true, "dark", ThemeOutcome::Ok(body.clone())),
        ),
        (
            "Theme deleted: dark",
            delete_output(false, "dark", ThemeOutcome::Ok(body.clone())),
            delete_output(true, "dark", ThemeOutcome::Ok(body.clone())),
        ),
    ] {
        assert_eq!(human, LeafOutput::Human(vec![leaf.to_string()]));
        let LeafOutput::Json(envelope) = json_output else {
            panic!("--json must emit a document for {leaf}");
        };


        assert_eq!(envelope["id"], json!("dark"));
        assert_eq!(envelope["applied"], body["applied"]);
    }
}

#[test]
fn dir_prints_the_path_in_human_mode_and_wraps_it_under_json() {
    assert_eq!(
        dir_output(false, "/cfg/domios/themes"),
        LeafOutput::Human(vec!["/cfg/domios/themes".to_string()])
    );
    let LeafOutput::Json(envelope) = dir_output(true, "/cfg/domios/themes") else {
        panic!("--json must emit a document");
    };

    assert_eq!(envelope["path"], json!("/cfg/domios/themes"));
}

#[test]
fn theme_directory_uses_the_domios_config_namespace() {
    assert_eq!(
        theme_dir_path(Some(std::path::PathBuf::from("/cfg"))),
        Some(std::path::PathBuf::from("/cfg/domios/themes"))
    );
    assert_eq!(theme_dir_path(None), None);
}

#[test]
fn save_without_apply_reports_the_backend_path_and_never_calls_the_apply_leg() {
    let saved = json!({"path": "/cfg/themes/dark.toml"});
    let calls = Cell::new(0);
    let human = save_output(
        false,
        "dark",
        false,
        ThemeOutcome::Ok(saved.clone()),
        || {
            calls.set(calls.get() + 1);
            ThemeOutcome::Ok(Value::Null)
        },
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(
        human,
        LeafOutput::Human(vec!["Theme saved: /cfg/themes/dark.toml".to_string()])
    );

    let LeafOutput::Json(envelope) = save_output(
        true,
        "dark",
        false,
        ThemeOutcome::Ok(saved.clone()),
        never_applies,
    ) else {
        panic!("--json must emit a document");
    };


    assert_eq!(envelope["id"], json!("dark"));
    assert_eq!(envelope["path"], json!("/cfg/themes/dark.toml"));
    assert_eq!(envelope["applied"], Value::Bool(false));
    assert_eq!(envelope["saved"], saved);
}

#[test]
fn a_failed_save_never_reaches_the_apply_leg() {
    let calls = Cell::new(0);
    let output = save_output(
        false,
        "dark",
        true,
        http_failure(StatusCode::INTERNAL_SERVER_ERROR),
        || {
            calls.set(calls.get() + 1);
            ThemeOutcome::Ok(Value::Null)
        },
    );
    assert_eq!(calls.get(), 0);
    assert!(matches!(output, LeafOutput::Failure { .. }));
}

#[test]
fn save_with_apply_reports_both_legs_when_both_succeed() {
    let saved = json!({"path": "/cfg/themes/dark.toml"});
    assert_eq!(
        save_output(false, "dark", true, ThemeOutcome::Ok(saved.clone()), || {
            ThemeOutcome::Ok(Value::Null)
        }),
        LeafOutput::Human(vec![
            "Theme saved: /cfg/themes/dark.toml".to_string(),
            "Theme applied: dark".to_string(),
        ])
    );
    let LeafOutput::Json(envelope) =
        save_output(true, "dark", true, ThemeOutcome::Ok(saved), || {
            ThemeOutcome::Ok(Value::Null)
        })
    else {
        panic!("--json must emit a document");
    };
    assert_eq!(envelope["applied"], Value::Bool(true));
}

#[test]
fn a_failing_apply_leg_after_a_good_save_fails_without_claiming_the_theme_was_applied() {
    let saved = json!({"path": "/cfg/themes/dark.toml"});
    let LeafOutput::Failure { earned, message } =
        save_output(false, "dark", true, ThemeOutcome::Ok(saved.clone()), || {
            http_failure(StatusCode::NOT_FOUND)
        })
    else {
        panic!("a failing apply leg must fail the command");
    };
    assert_eq!(
        earned,
        vec!["Theme saved: /cfg/themes/dark.toml".to_string()]
    );
    assert!(message.contains("/cfg/themes/dark.toml"));

    let LeafOutput::Failure { earned, .. } =
        save_output(true, "dark", true, ThemeOutcome::Ok(saved), || {
            http_failure(StatusCode::NOT_FOUND)
        })
    else {
        panic!("a failing apply leg must fail the command under --json too");
    };
    assert!(earned.is_empty(), "--json must emit one document only");
}

#[test]
fn save_falls_back_to_the_theme_id_when_the_backend_omits_a_path() {
    assert_eq!(
        save_output(
            false,
            "dark",
            false,
            ThemeOutcome::Ok(json!({})),
            never_applies
        ),
        LeafOutput::Human(vec!["Theme saved: dark".to_string()])
    );
}
