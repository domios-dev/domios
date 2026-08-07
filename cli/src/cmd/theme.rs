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
    Failure { earned: Vec<String>, message: String },
}

pub fn handle_theme(json: bool, action: ThemeAction) {
    let (base, _) = crate::read_api_config();
    let http = crate::build_local_api().unwrap_or_else(|error| {
        eprintln!("Error: failed to connect to local API: {error}");
        std::process::exit(1);
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
            for line in earned {
                println!("{line}");
            }
            if json {
                crate::cmd::plan_output::print_plan_error(true, &message);
            } else {
                eprintln!("Error: {message}");
            }
            std::process::exit(1);
        }
    }
}
