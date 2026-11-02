//! Bounded read of a tab's checklist for the owner UI and authorized agents.

use super::routes::{check_auth, AppState};
use super::tab_routes::caller_tab_header;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use codeterm_types::agent::{TabTodoChecklist, TabTodoWriteEntry};

#[derive(Debug, PartialEq, Eq)]
enum TodoReader {
    Owner(Option<String>),
    Parent(String),
}

fn reader_for(target: &str, caller: Option<String>) -> TodoReader {
    match caller {
        Some(caller) if caller != target => TodoReader::Parent(caller),
        caller => TodoReader::Owner(caller),
    }
}

fn read_error_status(error: &str) -> StatusCode {
    if error.starts_with("Tab not found") || error.ends_with("has no live workspace") {
        StatusCode::NOT_FOUND
    } else if error.starts_with("only ") {
        StatusCode::FORBIDDEN
    } else if error.contains("Storage is unavailable") {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

async fn tab_todos(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab_id): Path<String>,
) -> Result<Json<Option<TabTodoChecklist>>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, reason)| (status, reason.to_string()))?;
    let checklist = match reader_for(&tab_id, caller_tab_header(&headers)) {
        TodoReader::Owner(actor) => state.svc.tab_todo_list(&tab_id, actor.as_deref()).await,
        TodoReader::Parent(parent) => state.svc.tab_todo_parent_list(&tab_id, &parent).await,
    };
    checklist
        .map(Json)
        .map_err(|error| (read_error_status(&error), error))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/tabs/{id}/todos", get(tab_todos))
        .route(
            "/api/tabs/{id}/todos/review",
            axum::routing::post(review_todos),
        )
}

#[derive(serde::Deserialize)]
struct ReviewTodos {
    generation: u64,
    revision: u64,
    items: Option<Vec<TabTodoWriteEntry>>,
    note: Option<String>,
}

async fn review_todos(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab_id): Path<String>,
    Json(body): Json<ReviewTodos>,
) -> Result<Json<TabTodoChecklist>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, reason)| (status, reason.to_string()))?;
    let actor = caller_tab_header(&headers);
    state
        .svc
        .tab_todo_review_proposal(
            &tab_id,
            actor.as_deref(),
            body.items,
            body.note,
            Some((body.generation, body.revision)),
        )
        .await
        .map(Json)
        .map_err(|error| {
            (
                if error.starts_with("only ") {
                    StatusCode::FORBIDDEN
                } else {
                    StatusCode::CONFLICT
                },
                error,
            )
        })
}

#[cfg(test)]
mod tests;
