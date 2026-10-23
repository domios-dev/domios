use axum::{
    extract::{ConnectInfo, Request, State},
    http::{Method, StatusCode},
    response::{sse::Event, IntoResponse, Response, Sse},
    routing::any,
    Json, Router,
};
use codeterm_service_core::{
    config::ConfigState,
    startup::{RestApiControl, RestApiControlState, RestApiPhase, RestApiRuntimePolicy},
};
use parking_lot::Mutex;
use serde::Serialize;
use std::{collections::BTreeMap, convert::Infallible, net::SocketAddr, sync::Arc, time::Instant};
use tokio::sync::watch;
use tower::ServiceExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PhaseState {
    Starting,
    Ready,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Readiness {
    state: PhaseState,
    phases: BTreeMap<&'static str, PhaseState>,
}

struct Gate {
    router: Option<Router>,
    readiness: Readiness,
}

#[derive(Clone)]
pub(crate) struct EarlyApi {
    gate: Arc<Mutex<Gate>>,
    changes: watch::Sender<Readiness>,
    started: Instant,
    token: Option<String>,
}

pub(crate) struct PreparedStartup {
    pub config: ConfigState,
    pub control: RestApiControlState,
    pub api: EarlyApi,
}

impl PreparedStartup {
    pub fn new(frontend: Option<&url::Url>) -> Result<Self, String> {
        let config = super::init_config();
        let control = Arc::new(RestApiControl::new());
        let api_config = config.lock().api.clone();
        let api = EarlyApi::new(api_config.token.clone());
        codeterm_service_core::config::validate_api_trust_boundary(&api_config)?;
        let remote = codeterm_service_core::startup::rest_api_additional_bind(
            &api_config,
            cfg!(debug_assertions),
        )?;
        if !api_config.enabled {
            return Err("Server requires an enabled API listener".into());
        }
        let bind = codeterm_service_core::config::canonical_api_bind_addr(&api_config)?;
        control.listeners.configure(remote, Arc::new(|_| {}));
        control.listeners.set_proof_token(api_config.token.clone());
        control.set_runtime_policy(RestApiRuntimePolicy {
            enabled: true,
            cors: api_config.cors,
            cors_allow_origins: if api_config.cors {
                api_config.cors_allow_origins.clone()
            } else {
                Vec::new()
            },
        });
        let origins = codeterm_service_core::startup::frontend_shell_declared_frontend_origins(
            &api_config,
            frontend,
        );
        let router = api.router().layer(codeterm_api::routes::cors_layer(
            &origins,
            &control.runtime_policy().cors_allow_origins,
        ));
        codeterm_service_core::startup::rest_api::spawn(control.clone(), bind.to_string(), router);
        Ok(Self {
            config,
            control,
            api,
        })
    }

    pub async fn listening(&self) -> Result<SocketAddr, String> {
        let mut status = self.control.subscribe_status();
        tokio::time::timeout(crate::bootstrap::protocol::FAILURE_DEADLINE, async {
            loop {
                match status.borrow_and_update().phase {
                    RestApiPhase::Listening => {
                        let address = self
                            .control
                            .bound_addr()
                            .ok_or("Listener ownership unavailable")?;
                        self.api.complete("listener");
                        return Ok(address);
                    }
                    RestApiPhase::Failed => {
                        return Err("Server listener initialization failed".into())
                    }
                    RestApiPhase::Starting => {}
                }
                status.changed().await.map_err(|error| error.to_string())?;
            }
        })
        .await
        .map_err(|_| "Server listener readiness deadline".to_string())?
    }
}

impl EarlyApi {
    fn new(token: Option<String>) -> Self {
        let readiness = Readiness {
            state: PhaseState::Starting,
            phases: [
                "listener",
                "integrity",
                "states",
                "providers",
                "service",
                "plans",
                "background",
                "platform",
            ]
            .into_iter()
            .map(|name| (name, PhaseState::Starting))
            .collect(),
        };
        let (changes, _) = watch::channel(readiness.clone());
        Self {
            gate: Arc::new(Mutex::new(Gate {
                router: None,
                readiness,
            })),
            changes,
            started: Instant::now(),
            token,
        }
    }

    fn router(&self) -> Router {
        Router::new().fallback(any(route)).with_state(self.clone())
    }

    pub fn complete(&self, phase: &'static str) {
        let mut gate = self.gate.lock();
        gate.readiness.phases.insert(phase, PhaseState::Ready);
        self.publish(&gate.readiness, phase);
    }

    pub fn fail(&self, phase: &'static str) {
        let mut gate = self.gate.lock();
        gate.router = None;
        gate.readiness.state = PhaseState::Failed;
        gate.readiness.phases.insert(phase, PhaseState::Failed);
        self.publish(&gate.readiness, phase);
    }

    fn publish(&self, readiness: &Readiness, phase: &'static str) {
        self.changes.send_replace(readiness.clone());
        codeterm_service_core::startup::boot_timing::mark(
            "-",
            None,
            format!(
                "server_{phase}_{:?}",
                readiness.phases.get(phase).unwrap_or(&readiness.state)
            ),
        );
        log::debug!(
            "[boot-operation] server readiness phase={phase} state={:?} since_early_api_ms={}",
            readiness.phases.get(phase).unwrap_or(&readiness.state),
            self.started.elapsed().as_millis()
        );
    }

    pub fn activate(&self, router: Router) -> Result<(), String> {
        let mut gate = self.gate.lock();
        if gate.readiness.state != PhaseState::Starting
            || gate
                .readiness
                .phases
                .values()
                .any(|phase| *phase != PhaseState::Ready)
        {
            return Err("Service readiness prerequisites are incomplete".into());
        }
        gate.router = Some(router);
        gate.readiness.state = PhaseState::Ready;
        self.publish(&gate.readiness, "ready");
        Ok(())
    }
}

async fn route(State(api): State<EarlyApi>, request: Request) -> Response {
    let path = request.uri().path();
    let read = request.method() == Method::GET;
    let local = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_some_and(|peer| peer.0.ip().is_loopback());
    let authenticated = local
        || api.token.as_ref().is_some_and(|token| {
            request
                .headers()
                .get(axum::http::header::AUTHORIZATION)
                .is_some_and(|value| {
                    value.as_bytes().strip_prefix(b"Bearer ") == Some(token.as_bytes())
                })
        });
    if read && path == "/api/startup/events" {
        if !authenticated {
            return StatusCode::FORBIDDEN.into_response();
        }
        let mut changes = api.changes.subscribe();
        return Sse::new(async_stream::stream! {
            loop {
                let readiness = changes.borrow_and_update().clone();
                let terminal = readiness.state != PhaseState::Starting;
                let event = Event::default().event("startup-state").json_data(readiness);
                match event {
                    Ok(event) => yield Ok::<_, Infallible>(event),
                    Err(_) => break,
                }
                if terminal || changes.changed().await.is_err() { break; }
            }
        })
        .into_response();
    }
    let (router, readiness) = {
        let gate = api.gate.lock();
        (gate.router.clone(), gate.readiness.clone())
    };
    if let Some(router) = router {
        return router
            .oneshot(request)
            .await
            .unwrap_or_else(|error| match error {});
    }
    if read && path == "/api/health" {
        if !authenticated {
            return StatusCode::FORBIDDEN.into_response();
        }
        return Json(serde_json::json!({
            "status": if readiness.state == PhaseState::Failed { "error" } else { "ok" }, "startup": readiness,
            "version": codeterm_build_info::version(),
            "build": codeterm_build_info::hash(), "build_sha": codeterm_build_info::sha(),
            "apiVersion": codeterm_service_core::frontend_bundle_api_version::API_VERSION,
            "instance": {
                "name": std::env::var("CODETERM_INSTANCE").unwrap_or_default(),
                "api_port": std::env::var("CODETERM_API_PORT").unwrap_or_default(),
                "vite_port": std::env::var("CODETERM_VITE_PORT").unwrap_or_default(),
                "cdp_port": std::env::var("CODETERM_CDP_PORT").unwrap_or_default(),
                "identifier": std::env::var("CODETERM_DEV_IDENTIFIER").unwrap_or_default(),
            }
        })).into_response();
    }
    (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({
        "ok": false, "error": {"code": if readiness.state == PhaseState::Failed { "startup_failed" } else { "starting" }},
        "startup": readiness,
    }))).into_response()
}

#[cfg(test)]
mod tests;
