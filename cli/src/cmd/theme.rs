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
        ThemeAction::Save { id, file, apply } => {
            match crate::local_admission::read_local_text(&file) {
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
            }
        }
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

fn theme_value(id: Option<&str>, mut value: Value) -> Value {
    if let (Some(id), Some(object)) = (id, value.as_object_mut()) {
        object.insert("id".into(), Value::String(id.into()));
    }
    value
}

fn theme_dir() -> String {
    theme_dir_path(dirs::config_dir())
        .unwrap_or_default()
        .display()
        .to_string()
}

fn theme_dir_path(config_dir: Option<std::path::PathBuf>) -> Option<std::path::PathBuf> {
    config_dir.map(|dir| dir.join("domios").join("themes"))
}

fn list_output(json: bool, outcome: ThemeOutcome) -> LeafOutput {
    let value = match body(outcome) {
        Ok(value) => value,
        Err(failure) => return failure,
    };
    if json {
        return LeafOutput::Json(theme_value(None, value));
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
        LeafOutput::Json(theme_value(Some(name), value))
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
        LeafOutput::Json(theme_value(Some(id), value))
    } else {
        LeafOutput::Human(vec![format!("Theme deleted: {id}")])
    }
}

fn dir_output(json: bool, path: &str) -> LeafOutput {
    if json {
        LeafOutput::Json(theme_value(None, serde_json::json!({ "path": path })))
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
        return LeafOutput::Json(theme_value(
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
        LeafOutput::Json(value) => crate::cmd::print::print_json(&value),
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
mod tests;
