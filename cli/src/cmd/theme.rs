use serde_json::Value;

use crate::local_api::{LocalApiError, LocalApiResponse};
use crate::ThemeAction;

#[derive(Debug, PartialEq)]
enum ThemeOutcome {
    Ok(Value),
    Failed(String),
}

#[derive(Debug, PartialEq)]
enum LeafOutput {
    Human(Vec<String>),
    Json(Value),
    Failure {
        earned: Vec<String>,
        message: String,
    },
}

pub fn handle_theme(json: bool, action: ThemeAction) {
    let (base, _) = crate::read_api_config();
    let http = crate::build_local_api().unwrap_or_else(|error| {
        crate::cmd::print::fail(1, format!("failed to connect to local API: {error}"));
    });
    let output = match action {
        ThemeAction::List => list_output(
            json,
            classify(http.get(format!("{}/api/themes", base)).send()),
        ),
        ThemeAction::Apply { name } => apply_output(
            json,
            &name,
            classify(
                http.post(format!("{}/api/themes/apply", base))
                    .json(&serde_json::json!({ "name": name }))
                    .send(),
            ),
        ),
        ThemeAction::Save { id, file, apply } => match std::fs::read_to_string(&file) {
            Err(error) => LeafOutput::Failure {
                earned: Vec::new(),
                message: format!("reading {}: {}", file, error),
            },
            Ok(toml) => save_output(
                json,
                &id,
                apply,
                classify(
                    http.post(format!("{}/api/themes", base))
                        .json(&serde_json::json!({ "id": id, "toml": toml }))
                        .send(),
                ),
                || {
                    classify(
                        http.post(format!("{}/api/themes/apply", base))
                            .json(&serde_json::json!({ "name": id }))
                            .send(),
                    )
                },
            ),
        },
        ThemeAction::Delete { id } => delete_output(
            json,
            &id,
            classify(http.delete(format!("{}/api/themes/{}", base, id)).send()),
        ),
        ThemeAction::Dir => dir_output(json, &theme_dir()),
    };
    emit(json, output);
}

// `send()` returns Ok for any status; `error_for_status()` is the
// opt-in that turns a 4xx/5xx into a real failure.
fn classify(result: Result<LocalApiResponse, LocalApiError>) -> ThemeOutcome {
    match result {
        Ok(response) => match response.error_for_status() {
            Ok(response) => ThemeOutcome::Ok(response.json().unwrap_or_default()),
            Err(error) => ThemeOutcome::Failed(error.to_string()),
        },
        Err(error) => ThemeOutcome::Failed(error.to_string()),
    }
}

fn body(outcome: ThemeOutcome) -> Result<Value, LeafOutput> {
    match outcome {
        ThemeOutcome::Ok(value) => Ok(value),
        ThemeOutcome::Failed(message) => Err(LeafOutput::Failure {
            earned: Vec::new(),
            message,
        }),
    }
}

fn envelope(kind: &str, id: Option<&str>, data: Value) -> Value {
    serde_json::json!({ "ok": true, "kind": kind, "id": id, "data": data })
}

fn theme_dir() -> String {
    dirs::config_dir()
        .map(|dir| dir.join("codeterm").join("themes"))
        .unwrap_or_default()
        .display()
        .to_string()
}

fn list_output(json: bool, outcome: ThemeOutcome) -> LeafOutput {
    let value = match body(outcome) {
        Ok(value) => value,
        Err(failure) => return failure,
    };
    if json {
        return LeafOutput::Json(envelope("themes", None, value));
    }
    LeafOutput::Human(
        value
            .get("themes")
            .and_then(Value::as_array)
            .map(|themes| {
                themes
                    .iter()
                    .map(|theme| theme.as_str().unwrap_or("?").to_string())
                    .collect()
            })
            .unwrap_or_default(),
    )
}

fn apply_output(json: bool, name: &str, outcome: ThemeOutcome) -> LeafOutput {
    let value = match body(outcome) {
        Ok(value) => value,
        Err(failure) => return failure,
    };
    if json {
        LeafOutput::Json(envelope("theme", Some(name), value))
    } else {
        LeafOutput::Human(vec![format!("Theme applied: {name}")])
    }
}

fn delete_output(json: bool, id: &str, outcome: ThemeOutcome) -> LeafOutput {
    let value = match body(outcome) {
        Ok(value) => value,
        Err(failure) => return failure,
    };
    if json {
        LeafOutput::Json(envelope("theme", Some(id), value))
    } else {
        LeafOutput::Human(vec![format!("Theme deleted: {id}")])
    }
}

fn dir_output(json: bool, path: &str) -> LeafOutput {
    if json {
        LeafOutput::Json(envelope(
            "theme_dir",
            None,
            serde_json::json!({ "path": path }),
        ))
    } else {
        LeafOutput::Human(vec![path.to_string()])
    }
}

fn save_output(
    json: bool,
    id: &str,
    apply: bool,
    saved: ThemeOutcome,
    apply_theme: impl FnOnce() -> ThemeOutcome,
) -> LeafOutput {
    let saved = match body(saved) {
        Ok(value) => value,
        Err(failure) => return failure,
    };
    let path = saved
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or(id)
        .to_string();
    let mut earned = Vec::new();
    if !json {
        earned.push(format!("Theme saved: {path}"));
    }
    if apply {
        if let ThemeOutcome::Failed(message) = apply_theme() {
            return LeafOutput::Failure {
                earned,
                message: format!("theme saved to {path} but apply failed: {message}"),
            };
        }
        if !json {
            earned.push(format!("Theme applied: {id}"));
        }
    }
    if json {
        return LeafOutput::Json(envelope(
            "theme",
            Some(id),
            serde_json::json!({ "path": path, "applied": apply, "saved": saved }),
        ));
    }
    LeafOutput::Human(earned)
}

fn emit(json: bool, output: LeafOutput) {
    match output {
        LeafOutput::Human(lines) => {
            for line in lines {
                println!("{line}");
            }
        }
        LeafOutput::Json(value) => println!("{value}"),
        LeafOutput::Failure { earned, message } => {
            if !json {
                for line in earned {
                    println!("{line}");
                }
            }
            crate::cmd::print::fail(1, message);
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(envelope["ok"], Value::Bool(true));
        assert_eq!(envelope["kind"], json!("themes"));
        assert_eq!(envelope["id"], Value::Null);
        assert_eq!(envelope["data"], body);
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
            assert_eq!(envelope["ok"], Value::Bool(true));
            assert_eq!(envelope["kind"], json!("theme"));
            assert_eq!(envelope["id"], json!("dark"));
            assert_eq!(envelope["data"], body);
        }
    }

    #[test]
    fn dir_prints_the_path_in_human_mode_and_wraps_it_under_json() {
        assert_eq!(
            dir_output(false, "/cfg/codeterm/themes"),
            LeafOutput::Human(vec!["/cfg/codeterm/themes".to_string()])
        );
        let LeafOutput::Json(envelope) = dir_output(true, "/cfg/codeterm/themes") else {
            panic!("--json must emit a document");
        };
        assert_eq!(envelope["ok"], Value::Bool(true));
        assert_eq!(envelope["kind"], json!("theme_dir"));
        assert_eq!(envelope["id"], Value::Null);
        assert_eq!(envelope["data"]["path"], json!("/cfg/codeterm/themes"));
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
        assert_eq!(envelope["ok"], Value::Bool(true));
        assert_eq!(envelope["kind"], json!("theme"));
        assert_eq!(envelope["id"], json!("dark"));
        assert_eq!(envelope["data"]["path"], json!("/cfg/themes/dark.toml"));
        assert_eq!(envelope["data"]["applied"], Value::Bool(false));
        assert_eq!(envelope["data"]["saved"], saved);
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
        assert_eq!(envelope["data"]["applied"], Value::Bool(true));
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
}
