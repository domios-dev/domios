use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use codeterm_service_core::service::team_exchange::{
    self, ExchangeError, INVALID_REQUEST, NOT_FOUND,
};
use codeterm_service_core::service::team_template_recall;
use codeterm_team_package::contracts::TemplateRef;
use codeterm_team_package::{Package, PackageSkill};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::routes::AppState;

type ExchangeResult = Result<Json<Value>, (StatusCode, Json<ExchangeError>)>;

/// A bad request is 400, an unknown template 404 and a refused package 422 with diagnostics.
fn answer(outcome: Result<Value, ExchangeError>) -> ExchangeResult {
    outcome.map(Json).map_err(|error| {
        let status = match error.code {
            INVALID_REQUEST => StatusCode::BAD_REQUEST,
            NOT_FOUND => StatusCode::NOT_FOUND,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        };
        (status, Json(error))
    })
}

/// Package work reads and writes files; it runs off the async runtime.
async fn blocking(
    state: AppState,
    work: fn(
        &codeterm_service_core::service::ServiceContext,
        Option<&Value>,
    ) -> Result<Value, ExchangeError>,
    body: Value,
) -> ExchangeResult {
    let svc = state.svc.clone();
    let outcome = tokio::task::spawn_blocking(move || work(svc.as_ref(), Some(&body)))
        .await
        .unwrap_or_else(|error| Err(ExchangeError::invalid(error.to_string())));
    answer(outcome)
}

pub(super) async fn recall(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ExchangeResult {
    blocking(state, team_template_recall::recall, body).await
}

pub(super) async fn export(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ExchangeResult {
    blocking(state, team_exchange::export, body).await
}

pub(super) async fn import(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ExchangeResult {
    blocking(state, team_exchange::import, body).await
}

pub(super) async fn delete(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ExchangeResult {
    blocking(state, team_exchange::delete, body).await
}

pub(super) async fn card(State(state): State<AppState>, Json(body): Json<Value>) -> ExchangeResult {
    blocking(state, |_, params| team_exchange::share_card(params), body).await
}

pub(super) async fn distill(
    State(state): State<AppState>,
    Path(team_id): Path<String>,
    Json(mut body): Json<Value>,
) -> ExchangeResult {
    if let Some(object) = body.as_object_mut() {
        object.insert("team_id".into(), json!(team_id));
    }
    blocking(state, team_exchange::distill, body).await
}

/// The largest package file shown as text; bigger files are refused, never truncated.
const FILE_TEXT_LIMIT: usize = 256 * 1024;
const TOO_LARGE: &str = "too_large";
const NOT_TEXT: &str = "not_text";

#[derive(Deserialize)]
struct FileParams {
    template: TemplateRef,
    path: String,
}

fn refusal(code: &'static str, message: impl Into<String>) -> ExchangeError {
    ExchangeError {
        code,
        message: message.into(),
        diagnostics: Vec::new(),
    }
}

/// Package-relative only: no root, drive, backslash, or `.`/`..` segment.
fn package_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

/// A file the package carries: `team.yaml`, a declared or archived file, or an inline skill's file.
fn package_file(package: &Package, path: &str) -> Option<Vec<u8>> {
    if path == "team.yaml" {
        return Some(package.to_yaml().into_bytes());
    }
    if let Some(bytes) = package.file_bytes(path) {
        return Some(bytes.to_vec());
    }
    let (skill, rest) = path.split_once('/')?;
    package
        .document
        .skills
        .iter()
        .find_map(|entry| match entry {
            PackageSkill::Inline(inline) if inline.name == skill => inline
                .files
                .iter()
                .find(|file| file.path == rest)
                .map(|file| file.content.clone().into_bytes()),
            _ => None,
        })
}

fn read_file(params: Option<&Value>) -> Result<Value, ExchangeError> {
    let request: FileParams = serde_json::from_value(params.cloned().unwrap_or(Value::Null))
        .map_err(|error| ExchangeError::invalid(error.to_string()))?;
    if !package_relative(&request.path) {
        return Err(ExchangeError::invalid(
            "`path` must be a path inside the package",
        ));
    }
    let (package, _) = team_exchange::resolve(&request.template).ok_or_else(|| {
        refusal(
            NOT_FOUND,
            format!(
                "template `{}` is not builtin or in My templates",
                request.template.name
            ),
        )
    })?;
    let bytes = package_file(&package, &request.path).ok_or_else(|| {
        refusal(
            NOT_FOUND,
            format!("`{}` is not in the package", request.path),
        )
    })?;
    if bytes.len() > FILE_TEXT_LIMIT {
        return Err(refusal(
            TOO_LARGE,
            format!("`{}` is larger than 256 KiB", request.path),
        ));
    }
    let text = String::from_utf8(bytes)
        .ok()
        .filter(|text| !text.contains('\0'))
        .ok_or_else(|| refusal(NOT_TEXT, format!("`{}` is not a text file", request.path)))?;
    Ok(json!({ "path": request.path, "text": text }))
}

/// One package file as text, for reviewing a script before the owner keeps it.
pub(super) async fn file(State(state): State<AppState>, Json(body): Json<Value>) -> ExchangeResult {
    blocking(state, |_, params| read_file(params), body).await
}

#[cfg(test)]
mod tests {
    use super::package_relative;

    #[test]
    fn only_package_relative_paths_are_read() {
        for path in ["skills/check/scripts/fetch.py", "team.yaml", "a/b.md"] {
            assert!(package_relative(path), "{path}");
        }
        for path in [
            "",
            "/etc/passwd",
            "../x",
            "a/../b",
            "a//b",
            "./a",
            "C:/x",
            "a\\b",
        ] {
            assert!(!package_relative(path), "{path}");
        }
    }
}
