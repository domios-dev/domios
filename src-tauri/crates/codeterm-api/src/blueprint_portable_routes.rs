use super::{
    auth::{caller_access_level, caller_peer_id},
    routes::AppState,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_blueprint_types::{blueprint_document::Document, blueprint_recipe::Recipe};
use codeterm_sentinel_host::BlueprintPortableExt;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportBody {
    workspace_id: String,
    team_id: Value,
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportBody {
    caller_tab_id: String,
    recipe: Value,
    assignments: Vec<String>,
    scope: Option<String>,
}

fn host_access(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    caller_access_level(state, headers)?;
    if caller_peer_id(state, headers).is_some() {
        return Err((
            StatusCode::FORBIDDEN,
            "peer-origin Blueprint sharing is unavailable".into(),
        ));
    }
    Ok(())
}

pub async fn export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ExportBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    host_access(&state, &headers)?;
    let team = codeterm_sentinel_host::service_adapter::graph::explicit_team(&body.team_id)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    tokio::task::spawn_blocking(move || {
        let recipe =
            state
                .svc
                .blueprint_export_host(&body.workspace_id, team.as_deref(), &body.id)?;
        let document = Document::from_recipe(&recipe)?;
        Ok::<_, String>(json!({"name":document.name,"yaml":document.yaml()?}))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

fn import_recipe(source: &Value, assignments: &[String]) -> Result<Recipe, String> {
    let raw = match source {
        Value::String(raw) => raw.clone(),
        value => serde_json::to_string(value).map_err(|error| error.to_string())?,
    };
    let checked =
        codeterm_blueprint_types::blueprint_document::check("import.yaml", &raw, assignments);
    if !checked.ok {
        return Err(checked
            .diagnostics
            .iter()
            .filter(|d| d.severity == "error")
            .map(|d| {
                format!(
                    "{}:{}:{}: {}: {} (fix: {})",
                    d.file, d.line, d.column, d.field, d.message, d.hint
                )
            })
            .collect::<Vec<_>>()
            .join("\n"));
    }
    Ok(Recipe {
        schema_version: 2,
        definition: checked.definition.ok_or("Missing checked Blueprint")?,
    })
}

pub async fn import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ImportBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    host_access(&state, &headers)?;
    let caller =
        crate::auth::caller_claim(&state, &headers, Some(&body.caller_tab_id))?.ok_or((
            StatusCode::FORBIDDEN,
            "Blueprint import requires a tab credential".into(),
        ))?;
    tokio::task::spawn_blocking(move || {
        let recipe = import_recipe(&body.recipe, &body.assignments)?;
        state.svc.blueprint_import_host(&caller, recipe, &[], body.scope.as_deref())
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

#[cfg(test)]
mod tests;
