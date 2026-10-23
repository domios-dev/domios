use axum::{http::StatusCode, response::IntoResponse, Json};

pub(super) fn map_metadata_error(error: String) -> axum::response::Response {
    use codeterm_service_core::service::billing::{FREE_LIMIT_REACHED, PLAN_LIMIT_LOCKED};
    if let Some(code) = [PLAN_LIMIT_LOCKED, FREE_LIMIT_REACHED]
        .into_iter()
        .find(|code| error.starts_with(code))
    {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "ok": false, "error": { "code": code, "message": error }
            })),
        )
            .into_response();
    }
    let status = if error.starts_with("Tab not found") {
        StatusCode::NOT_FOUND
    } else if error.starts_with("description must be")
        || error == codeterm_service_core::service::tab::TAB_TITLE_ERROR
    {
        StatusCode::BAD_REQUEST
    } else if error.starts_with("task-bound rename refused")
        || error.starts_with("Cannot rename protected")
    {
        StatusCode::FORBIDDEN
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, error).into_response()
}
