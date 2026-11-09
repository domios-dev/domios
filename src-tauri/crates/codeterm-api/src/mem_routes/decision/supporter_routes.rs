use super::connection_routes::ConnectionState;
use super::supporter_json::SupporterJson;
use super::*;
use axum::extract::FromRef;
use codeterm_service_core::service::supporter::connections;
use codeterm_service_core::service::supporter::responses::{
    SupporterSpend, SupporterState, SupporterStatus, SupporterTokenStatus,
};

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
    AxumState(state): AxumState<ConnectionState>,
    Query(query): Query<AdapterQuery>,
) -> RouteResult {
    let id = query
        .adapter
        .or_else(|| state.config.lock().supporter.adapter.clone())
        .ok_or_else(no_active)?;
    let mode = state
        .registry
        .get(&id)
        .ok_or_else(|| {
            super::connection_routes::error(connections::ConnectionError {
                code: "unknown_adapter",
                message: "This adapter is not registered.",
            })
        })?
        .token_mode();
    token_io(move || {
        let status = state.store.status(&id)?;
        Ok(json!(token_status_json(&id, &status, mode)))
    })
    .await
    .map(Json)
}

pub async fn supporter_status(AxumState(state): AxumState<ConnectionState>) -> RouteResult {
    token_io(move || {
        let cfg = state.config.lock().supporter.clone();
        let models = &state.registry;
        let runtime = state.runtime;
        let store = state.store;
        let mut adapters = registered_adapters(models);
        for adapter in &mut adapters {
            adapter.token = Some(token_status_json(
                &adapter.id,
                &store.status(&adapter.id)?,
                adapter.token_mode,
            ));
        }
        let id = cfg.active_id().unwrap_or("");
        let selected_token = store.status(id)?;
        let selected = models.get(id);
        let mode = selected
            .as_ref()
            .map_or(TokenMode::None, |model| model.token_mode());
        let token =
            codeterm_service_core::service::supporter::read_token_snapshot(&runtime, &store, id);
        let connected =
            codeterm_service_core::service::supporter::supporter_connected(&cfg, models, &token);
        let rows = connections::list(&cfg, models, &store)
            .map_err(|_| io::Error::other("connections unavailable"))?;
        let mut state = supporter_state(
            cfg.active_id().is_some(),
            &selected_token,
            selected.is_some(),
            mode,
        );
        if !connected && state == SupporterState::Active {
            state = SupporterState::AdapterUnavailable;
        }
        let price = selected.as_ref().and_then(|model| model.pricing());
        let spend = spend_snapshot(&runtime, id, price.as_ref());
        let activity = codeterm_service_core::service::supporter::activity_status(
            &runtime,
            cfg.calls_per_hour.unwrap_or(0),
        );
        if connected && activity.budget.reached {
            state = SupporterState::BudgetReached;
        }
        if connected && spend.exhausted {
            state = SupporterState::CreditExhausted;
        }
        let output = SupporterStatus {
            state,
            token_mode: mode,
            adapter: cfg.adapter.clone(),
            model: rows
                .iter()
                .find(|row| row.active)
                .and_then(|row| row.model.clone()),
            deadline_ms: cfg.deadline_ms,
            connected,
            token: token_status_json(id, &selected_token, mode),
            adapter_registered: selected.is_some(),
            adapters,
            connections: rows,
            spend,
            pricing: price,
            activity,
            observation_scheduler:
                codeterm_service_core::service::supporter::observation_scheduler_status(),
        };
        Ok(json!(output))
    })
    .await
    .map(Json)
}

pub async fn supporter_models(AxumState(state): AxumState<ConnectionState>) -> RouteResult {
    token_io(move || {
        let cfg = state.config.lock().supporter.clone();
        let model = cfg.active_id().and_then(|id| cfg.connection(id).model.or_else(|| state.registry.get(id).and_then(|adapter| adapter.cached_models().first().map(|model| model.id.clone()))));
        Ok(json!({ "adapter": cfg.adapter, "model": model, "adapters": registered_adapters(&state.registry) }))
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

fn token_status_json(adapter: &str, status: &TokenStatus, mode: TokenMode) -> SupporterTokenStatus {
    SupporterTokenStatus {
        adapter: adapter.into(),
        token_mode: mode,
        present: status.present,
        path: status.path.clone(),
        mode_ok: status.mode_ok,
    }
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
    runtime: &codeterm_service_core::service::supporter::SupporterRuntime,
    adapter: &str,
    price: Option<&codeterm_decision::DecisionPricing>,
) -> SupporterSpend {
    let mut spend = runtime.spend_status(adapter);
    if price.is_none() {
        spend.today.estimated_usd = None;
        spend.month.estimated_usd = None;
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
    let runtime = codeterm_service_core::service::supporter::runtime_with_notifications(&state.svc);
    token_io(move || feed_snapshot(&runtime, query))
        .await
        .map(Json)
}

fn feed_snapshot(
    runtime: &codeterm_service_core::service::supporter::SupporterRuntime,
    query: SupporterFeedQuery,
) -> io::Result<Value> {
    runtime
        .feed(query.limit.unwrap_or(50), query.team_id.as_deref())
        .map(|entries| json!({ "entries": entries }))
}

#[cfg(test)]
mod tests;
