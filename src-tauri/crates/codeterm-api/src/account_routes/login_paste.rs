use super::{admit, unavailable, AppState, CallerIdentity, Error};
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    Extension, Json,
};

pub(super) fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/api/account/login/paste",
            axum::routing::post(start).get(pending).put(complete),
        )
        .layer(axum::extract::DefaultBodyLimit::max(
            super::MAX_CALLBACK_REQUEST_BYTES,
        ))
}

async fn start(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<(HeaderMap, Json<serde_json::Value>), Error> {
    admit(caller.as_ref())?;
    let (id, url) = state
        .svc
        .account_login_paste_start()
        .await
        .map_err(unavailable)?;
    Ok(reply(id, url))
}

fn reply(id: String, url: url::Url) -> (HeaderMap, Json<serde_json::Value>) {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    (
        headers,
        Json(serde_json::json!({"login_id": id, "url": url.as_str()})),
    )
}

async fn pending(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<(HeaderMap, Json<serde_json::Value>), Error> {
    admit(caller.as_ref())?;
    let (id, url) = state.svc.account_login_paste_pending().await.ok_or((
        StatusCode::NOT_FOUND,
        "No pending browserless sign-in; run domios account login --no-browser first".into(),
    ))?;
    Ok(reply(id, url))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    login_id: String,
    url: String,
}

async fn complete(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    Json(input): Json<Input>,
) -> Result<Json<codeterm_account::Status>, Error> {
    admit(caller.as_ref())?;
    let callback = codeterm_account::Callback::parse_paste(&input.url)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    state
        .svc
        .account_login_paste_complete(&input.login_id, callback)
        .await
        .map(Json)
        .map_err(unavailable)
}
