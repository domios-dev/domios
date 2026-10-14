use super::connection_routes::ConnectionState;
use super::supporter_json::SupporterJson;
use super::*;
use crate::service::supporter::connections;
use axum::extract::FromRef;

#[derive(Deserialize)]
pub struct TokenSetBody {
    adapter: Option<String>,
    token: String,
}
#[derive(Deserialize)]
pub struct AdapterQuery {
    adapter: Option<String>,
}
#[derive(Deserialize)]
pub struct SupporterUseBody {
    adapter: String,
    model: Option<String>,
}

type RouteResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn json_error(code: &str, message: &str) -> Json<Value> {
    Json(json!({ "error": { "code": code, "message": message } }))
}
fn no_active() -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        json_error(
            "unknown_adapter",
            "Specify an adapter; there is no active connection.",
        ),
    )
}

pub async fn supporter_token_set(
    AxumState(state): AxumState<AppState>,
    SupporterJson(body): SupporterJson<TokenSetBody>,
) -> RouteResult {
    let id = body
        .adapter
        .or_else(|| state.config.lock().supporter.adapter.clone())
        .ok_or_else(no_active)?;
    super::connection_routes::mutate(ConnectionState::from_ref(&state), move |service| {
        service.replace_token(&id, &body.token)
    })
    .await
}

pub async fn supporter_token_clear(
    AxumState(state): AxumState<AppState>,
    Query(query): Query<AdapterQuery>,
) -> RouteResult {
    let id = query
        .adapter
        .or_else(|| state.config.lock().supporter.adapter.clone())
        .ok_or_else(no_active)?;
    super::connection_routes::mutate(ConnectionState::from_ref(&state), move |service| {
        service.clear_token(&id)
    })
    .await
}

pub async fn supporter_token_status(
    AxumState(state): AxumState<AppState>,
    Query(query): Query<AdapterQuery>,
) -> RouteResult {
    let id = query
        .adapter
        .or_else(|| state.config.lock().supporter.adapter.clone())
        .ok_or_else(no_active)?;
    let mode = state
        .svc
        .supporter_models
        .get(&id)
        .ok_or_else(|| {
            super::connection_routes::error(connections::ConnectionError {
                code: "unknown_adapter",
                message: "This adapter is not registered.",
            })
        })?
        .token_mode();
    token_io(move || {
        let _transaction = connections::TRANSACTION.lock();
        let status = token_store().status(&id)?;
        Ok(token_status_json(&id, &status, mode))
    })
    .await
    .map(Json)
}

pub async fn supporter_status(AxumState(state): AxumState<AppState>) -> RouteResult {
    token_io(move || {
        let _transaction = connections::TRANSACTION.lock();
        let cfg = state.config.lock().supporter.clone();
        let models = &state.svc.supporter_models;
        let runtime = crate::service::supporter::runtime_with_notifications(&state.svc);
        let store = token_store();
        let mut adapters = registered_adapters(models);
        for adapter in &mut adapters {
            if let Some(id) = adapter["id"].as_str() {
                let mode = models.get(id).map_or(TokenMode::None, |model| model.token_mode());
                adapter["token"] = token_status_json(id, &store.status(id)?, mode);
            }
        }
        let id = cfg.active_id().unwrap_or("");
        let selected_token = store.status(id)?;
        let selected = models.get(id);
        let mode = selected.as_ref().map_or(TokenMode::None, |model| model.token_mode());
        let token = crate::service::supporter::read_token_snapshot(&runtime, &store, id);
        let connected = crate::service::supporter::supporter_connected(&cfg, models, &token);
        let rows = connections::list(&cfg, models, &store).map_err(|_| io::Error::other("connections unavailable"))?;
        let mut output = json!({
            "state": supporter_state(cfg.active_id().is_some(), &selected_token, selected.is_some(), mode),
            "token_mode": token_mode_name(mode), "adapter": cfg.adapter,
            "model": rows.iter().find(|row| row["active"] == true).map(|row| row["model"].clone()),
            "deadline_ms": cfg.deadline_ms, "connected": connected,
            "token": token_status_json(id, &selected_token, mode),
            "adapter_registered": selected.is_some(), "adapters": adapters, "connections": rows,
        });
        if !connected && output["state"] == "active" { output["state"] = json!("adapter_unavailable"); }
        let price = selected.as_ref().and_then(|model| model.pricing());
        output["spend"] = spend_snapshot(&runtime, id, price.as_ref());
        output["pricing"] = price.as_ref().map_or(Value::Null, |price| json!(price));
        let activity = crate::service::supporter::activity_status(&runtime, cfg.calls_per_hour.unwrap_or(0));
        if connected && activity["budget"]["reached"] == true { output["state"] = json!("budget_reached"); }
        if connected && output["spend"]["exhausted"] == true { output["state"] = json!("credit_exhausted"); }
        if let (Some(output), Some(activity)) = (output.as_object_mut(), activity.as_object()) { output.extend(activity.clone()); }
        Ok(output)
    }).await.map(Json)
}

pub async fn supporter_models(AxumState(state): AxumState<AppState>) -> RouteResult {
    token_io(move || {
        let _transaction = connections::TRANSACTION.lock();
        let cfg = state.config.lock().supporter.clone();
        let model = cfg.active_id().and_then(|id| cfg.connection(id).model.or_else(|| state.svc.supporter_models.get(id).and_then(|adapter| adapter.models().first().map(|model| model.id.clone()))));
        Ok(json!({ "adapter": cfg.adapter, "model": model, "adapters": registered_adapters(&state.svc.supporter_models) }))
    }).await.map(Json)
}

pub async fn supporter_use(
    AxumState(state): AxumState<AppState>,
    SupporterJson(body): SupporterJson<SupporterUseBody>,
) -> RouteResult {
    super::connection_routes::mutate(ConnectionState::from_ref(&state), move |service| {
        service.use_adapter(&body.adapter, body.model)
    })
    .await
}

fn token_status_json(adapter: &str, status: &TokenStatus, mode: TokenMode) -> Value {
    json!({
        "adapter": adapter,
        "token_mode": token_mode_name(mode),
        "present": status.present,
        "path": status.path,
        "mode_ok": status.mode_ok,
    })
}

/// Token file IO stays off the async runtime; io errors never carry the token value.
async fn token_io<T, F>(work: F) -> Result<T, (StatusCode, Json<Value>)>
where
    T: Send + 'static,
    F: FnOnce() -> io::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json_error(
                    "unavailable",
                    "Supporter storage task failed. Refresh and retry.",
                ),
            )
        })?
        .map_err(|error| {
            let status = if error.kind() == io::ErrorKind::InvalidInput {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                status,
                json_error(
                    "unavailable",
                    "Supporter storage is unavailable. Refresh and retry.",
                ),
            )
        })
}

fn spend_snapshot(
    runtime: &crate::service::supporter::SupporterRuntime,
    adapter: &str,
    price: Option<&codeterm_decision::DecisionPricing>,
) -> Value {
    let mut spend = runtime.spend_status(adapter);
    if price.is_none() {
        for period in ["today", "month"] {
            spend[period]["estimated_usd"] = Value::Null;
        }
    }
    spend
}

#[derive(Deserialize)]
pub struct SupporterFeedQuery {
    limit: Option<usize>,
    team_id: Option<String>,
}

pub async fn supporter_feed(
    AxumState(state): AxumState<AppState>,
    Query(query): Query<SupporterFeedQuery>,
) -> RouteResult {
    let runtime = crate::service::supporter::runtime_with_notifications(&state.svc);
    token_io(move || feed_snapshot(&runtime, query))
        .await
        .map(Json)
}

fn feed_snapshot(
    runtime: &crate::service::supporter::SupporterRuntime,
    query: SupporterFeedQuery,
) -> io::Result<Value> {
    runtime
        .feed(query.limit.unwrap_or(50), query.team_id.as_deref())
        .map(|entries| json!({ "entries": entries }))
}

#[cfg(test)]
mod tests;
