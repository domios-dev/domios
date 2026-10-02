use super::*;

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

type RouteResult = Result<Json<Value>, (StatusCode, String)>;

pub async fn supporter_token_set(
    AxumState(state): AxumState<AppState>,
    Json(body): Json<TokenSetBody>,
) -> RouteResult {
    let adapter = body
        .adapter
        .unwrap_or_else(|| state.config.lock().supporter.adapter.clone());
    let Some(model) = state.svc.supporter_models.get(&adapter) else {
        return Err(unknown_adapter(&state.svc.supporter_models, &adapter));
    };
    if model.token_mode() == TokenMode::None {
        return Ok(Json(no_token_needed_json(&adapter)));
    }
    let token = body.token;
    let status = token_io(move || {
        let store = token_store();
        store.set(&adapter, &token)?;
        store.status(&adapter).map(|status| (adapter, status))
    })
    .await?;
    Ok(Json(token_status_json(
        &status.0,
        &status.1,
        model.token_mode(),
    )))
}

pub async fn supporter_token_clear(
    AxumState(state): AxumState<AppState>,
    Query(query): Query<AdapterQuery>,
) -> RouteResult {
    let adapter = query
        .adapter
        .unwrap_or_else(|| state.config.lock().supporter.adapter.clone());
    if state
        .svc
        .supporter_models
        .get(&adapter)
        .is_some_and(|model| model.token_mode() == TokenMode::None)
    {
        return Ok(Json(no_token_needed_json(&adapter)));
    }
    let (adapter, removed) = token_io(move || {
        let removed = token_store().clear(&adapter)?;
        Ok((adapter, removed))
    })
    .await?;
    let token_mode = state
        .svc
        .supporter_models
        .get(&adapter)
        .map(|model| model.token_mode())
        .unwrap_or(TokenMode::Required);
    Ok(Json(json!({
        "adapter": adapter,
        "removed": removed,
        "token_mode": token_mode_name(token_mode),
    })))
}

pub async fn supporter_token_status(
    AxumState(state): AxumState<AppState>,
    Query(query): Query<AdapterQuery>,
) -> RouteResult {
    let adapter = query
        .adapter
        .unwrap_or_else(|| state.config.lock().supporter.adapter.clone());
    if state
        .svc
        .supporter_models
        .get(&adapter)
        .is_some_and(|model| model.token_mode() == TokenMode::None)
    {
        return Ok(Json(no_token_needed_json(&adapter)));
    }
    let (adapter, status) = token_io(move || {
        let status = token_store().status(&adapter)?;
        Ok((adapter, status))
    })
    .await?;
    let token_mode = state
        .svc
        .supporter_models
        .get(&adapter)
        .map(|model| model.token_mode())
        .unwrap_or(TokenMode::Required);
    Ok(Json(token_status_json(&adapter, &status, token_mode)))
}

pub async fn supporter_status(AxumState(state): AxumState<AppState>) -> RouteResult {
    let models = state.svc.supporter_models.clone();
    let cfg = state.config.lock().supporter.clone();
    token_io(move || {
        let store = token_store();
        let mut adapters = registered_adapters(&models);
        for adapter in &mut adapters {
            if let Some(id) = adapter.get("id").and_then(Value::as_str) {
                let status = store.status(id)?;
                let mode = models
                    .get(id)
                    .map(|model| model.token_mode())
                    .unwrap_or(TokenMode::Required);
                adapter["token"] = token_status_json(id, &status, mode);
            }
        }
        let selected_token = store.status(&cfg.adapter)?;
        let selected_model = models.get(&cfg.adapter);
        let adapter_registered = selected_model.is_some();
        let token_mode = selected_model.map_or(TokenMode::Required, |model| model.token_mode());
        let mut state = supporter_state(
            !cfg.adapter.is_empty(),
            &selected_token,
            adapter_registered,
            token_mode,
        );
        let connected = crate::service::supporter::supporter_connected(
            &cfg,
            &models,
            &crate::service::supporter::TokenSnapshot::read(&store, &cfg.adapter),
        );
        if !connected && state == "active" {
            state = "token_unusable";
        }
        let model = cfg.model.clone().or_else(|| {
            models
                .get(&cfg.adapter)
                .and_then(|model| model.models().first().map(|entry| entry.id.clone()))
        });
        let mut output = json!({
            "state": state,
            "token_mode": token_mode_name(token_mode),
            "adapter": cfg.adapter,
            "model": model,
            "deadline_ms": cfg.deadline_ms,
            "connected": connected,
            "token": token_status_json(&cfg.adapter, &selected_token, token_mode),
            "adapter_registered": adapter_registered,
            "adapters": adapters,
        });
        let activity = crate::service::supporter::activity_status(
            &crate::service::supporter::runtime(),
            cfg.calls_per_hour,
        );
        if output["connected"] == true && activity["budget"]["reached"] == true {
            output["state"] = json!("budget_reached");
        }
        if let (Some(output), Some(activity)) = (output.as_object_mut(), activity.as_object()) {
            output.extend(activity.clone());
        }
        Ok(output)
    })
    .await
    .map(Json)
}

pub async fn supporter_models(AxumState(state): AxumState<AppState>) -> RouteResult {
    let cfg = state.config.lock().supporter.clone();
    let models = state.svc.supporter_models.clone();
    let adapters = registered_adapters(&models);
    Ok(Json(json!({
        "adapter": cfg.adapter,
        "model": cfg.model,
        "adapters": adapters,
    })))
}

pub async fn supporter_use(
    AxumState(state): AxumState<AppState>,
    Json(body): Json<SupporterUseBody>,
) -> RouteResult {
    let Some(adapter) = state.svc.supporter_models.get(&body.adapter) else {
        return Err(unknown_adapter(&state.svc.supporter_models, &body.adapter));
    };
    let available = adapter.models();
    let current = state.config.lock().supporter.clone();
    let selected_model = select_model_id(
        &body.adapter,
        &available,
        body.model,
        current.adapter == body.adapter,
        current.model,
    )
    .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    if let Some(model_id) = selected_model.as_deref() {
        if !adapter.select_model(model_id) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("adapter {} cannot select model {model_id}", body.adapter),
            ));
        }
    }
    let adapter_id = body.adapter;
    let cfg = update_supporter_config(state, move |cfg| {
        cfg.adapter = adapter_id;
        cfg.model = selected_model;
    })
    .await?;
    Ok(Json(json!({ "adapter": cfg.adapter, "model": cfg.model })))
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

fn no_token_needed_json(adapter: &str) -> Value {
    json!({
        "adapter": adapter,
        "token_mode": "none",
        "message": "this adapter needs no token",
        "present": false,
    })
}

fn unknown_adapter(models: &SupporterRegistry, adapter: &str) -> (StatusCode, String) {
    let mut registered = models.ids();
    registered.sort();
    (
        StatusCode::NOT_FOUND,
        format!(
            "supporter adapter is not registered: {adapter}; registered adapters: {}",
            if registered.is_empty() {
                "none".to_owned()
            } else {
                registered.join(", ")
            }
        ),
    )
}

async fn update_supporter_config<F>(
    state: AppState,
    update: F,
) -> Result<crate::config::schema::SupporterConfig, (StatusCode, String)>
where
    F: FnOnce(&mut crate::config::schema::SupporterConfig) + Send + 'static,
{
    let config_state = state.config.clone();
    tokio::task::spawn_blocking(move || {
        let mut config = config_state.lock().clone();
        update(&mut config.supporter);
        crate::config::bounds::validate_app_config(&config)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let supporter = config.supporter.clone();
        crate::config::save_config_and_apply(&config_state, config)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        Ok::<_, io::Error>(supporter)
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("supporter config task failed: {error}"),
        )
    })?
    .map_err(|error| {
        let status = if error.kind() == io::ErrorKind::InvalidInput {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (status, format!("supporter config update failed: {error}"))
    })
}

/// Token file IO stays off the async runtime; io errors never carry the token value.
async fn token_io<T, F>(work: F) -> Result<T, (StatusCode, String)>
where
    T: Send + 'static,
    F: FnOnce() -> io::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("supporter token task failed: {error}"),
            )
        })?
        .map_err(|error| {
            let status = if error.kind() == io::ErrorKind::InvalidInput {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, format!("supporter token store: {error}"))
        })
}
