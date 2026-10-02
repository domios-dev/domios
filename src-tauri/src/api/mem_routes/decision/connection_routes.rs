use super::supporter_json::SupporterJson;
use super::*;
use crate::service::supporter::connections::{self, ConnectionError, Connections};
use axum::extract::{FromRef, Path};

type Result = std::result::Result<Json<Value>, (StatusCode, Json<Value>)>;

type Save = Box<dyn FnMut(&SupporterConfig) -> std::result::Result<(), ConnectionError>>;
type ConnectionService<'a> = Connections<'a, Save>;

type Persist =
    Arc<dyn Fn(&SupporterConfig) -> std::result::Result<(), ConnectionError> + Send + Sync>;

#[derive(Clone)]
pub struct ConnectionState {
    config: crate::config::ConfigState,
    registry: Arc<SupporterRegistry>,
    store: TokenStore,
    runtime: Arc<crate::service::supporter::SupporterRuntime>,
    persist: Persist,
}

impl FromRef<AppState> for ConnectionState {
    fn from_ref(state: &AppState) -> Self {
        let config = state.config.clone();
        let config_state = config.clone();
        let persist: Persist = Arc::new(move |supporter| {
            let mut next = config_state.lock().clone();
            next.supporter = supporter.clone();
            crate::config::bounds::validate_app_config(&next)
                .map_err(|_| ConnectionError::unavailable())?;
            crate::config::save_config_and_apply(&config_state, next)
                .map_err(|_| ConnectionError::unavailable())
        });
        Self {
            config,
            registry: state.svc.supporter_models.clone(),
            store: token_store(),
            runtime: crate::service::supporter::runtime_with_notifications(&state.svc),
            persist,
        }
    }
}

#[derive(Deserialize)]
pub struct ConnectBody {
    adapter: String,
    model: Option<String>,
    token: Option<String>,
    activate: bool,
}

#[derive(Deserialize)]
pub struct ModelBody {
    model: String,
}

#[derive(Deserialize)]
pub struct ReplaceTokenBody {
    token: String,
}

pub(super) fn error(error: ConnectionError) -> (StatusCode, Json<Value>) {
    let status = match error.code {
        "unknown_adapter" => StatusCode::NOT_FOUND,
        "invalid_model" | "invalid_request" => StatusCode::BAD_REQUEST,
        "authentication_failed" => StatusCode::UNAUTHORIZED,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    (status, Json(json!({ "error": error })))
}

pub(super) async fn mutate<F>(state: ConnectionState, work: F) -> Result
where
    F: FnOnce(&mut ConnectionService<'_>) -> std::result::Result<Value, ConnectionError>
        + Send
        + 'static,
{
    tokio::task::spawn_blocking(move || {
        let _transaction = connections::TRANSACTION.lock();
        let mut cfg = state.config.lock().supporter.clone();
        let save = state.persist.clone();
        let persist = Box::new(move |supporter: &SupporterConfig| save(supporter)) as Save;
        let mut service = Connections {
            cfg: &mut cfg,
            registry: &state.registry,
            store: &state.store,
            persist,
        };
        let result = work(&mut service);
        state.runtime.connection_changed();
        result
    })
    .await
    .map_err(|_| error(ConnectionError::unavailable()))?
    .map(Json)
    .map_err(error)
}

pub async fn connections_list(AxumState(state): AxumState<ConnectionState>) -> Result {
    tokio::task::spawn_blocking(move || {
        let _transaction = connections::TRANSACTION.lock();
        let cfg = state.config.lock().supporter.clone();
        connections::list(&cfg, &state.registry, &state.store)
            .map(|rows| json!({ "connections": rows, "active": cfg.adapter }))
    })
    .await
    .map_err(|_| error(ConnectionError::unavailable()))?
    .map(Json)
    .map_err(error)
}

pub async fn connections_connect(
    AxumState(state): AxumState<ConnectionState>,
    SupporterJson(body): SupporterJson<ConnectBody>,
) -> Result {
    if !body.activate {
        return Err(error(ConnectionError { code: "invalid_request", message: "Connect requires activate: true. Use the model endpoint to update a saved connection." }));
    }
    mutate(state, move |service| {
        service.connect(&body.adapter, body.model, body.token.as_deref(), true)
    })
    .await
}

pub async fn connections_activate(
    AxumState(state): AxumState<ConnectionState>,
    Path(adapter): Path<String>,
) -> Result {
    mutate(state, move |service| service.activate(&adapter)).await
}

pub async fn connections_model(
    AxumState(state): AxumState<ConnectionState>,
    Path(adapter): Path<String>,
    SupporterJson(body): SupporterJson<ModelBody>,
) -> Result {
    mutate(state, move |service| {
        service.set_model(&adapter, body.model)
    })
    .await
}

pub async fn connections_token(
    AxumState(state): AxumState<ConnectionState>,
    Path(adapter): Path<String>,
    SupporterJson(body): SupporterJson<ReplaceTokenBody>,
) -> Result {
    mutate(state, move |service| {
        service.replace_token(&adapter, &body.token)
    })
    .await
}

pub async fn connections_disconnect(
    AxumState(state): AxumState<ConnectionState>,
    Path(adapter): Path<String>,
) -> Result {
    mutate(state, move |service| service.disconnect(&adapter)).await
}

#[cfg(test)]
mod tests;
