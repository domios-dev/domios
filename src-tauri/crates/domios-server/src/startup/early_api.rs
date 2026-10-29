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
    desktop_ready: bool,
    routes_ready: bool,
    subsystems: BTreeMap<&'static str, PhaseState>,
    phases: BTreeMap<&'static str, PhaseState>,
}

struct Gate {
    router: Option<Router>,
    desktop: Option<Router>,
    readiness: Readiness,
}

#[derive(Clone)]
pub(crate) struct EarlyApi {
    gate: Arc<Mutex<Gate>>,
    changes: watch::Sender<Readiness>,
    started: Instant,
    token: Option<String>,
    diagnostics_port: codeterm_api::health_routes::NativeDiagnosticsPort,
    frontend_identity: codeterm_api::health_routes::NativeFrontendIdentity,
}

pub(crate) struct PreparedStartup {
    pub config: ConfigState,
    pub control: RestApiControlState,
    pub api: EarlyApi,
}

impl PreparedStartup {
    pub fn new(
        frontend: Option<&url::Url>,
        diagnostics_port: codeterm_api::health_routes::NativeDiagnosticsPort,
        frontend_identity: codeterm_api::health_routes::NativeFrontendIdentity,
    ) -> Result<Self, String> {
        let config = super::init_config();
        let control = Arc::new(RestApiControl::new());
        let api_config = config.lock().api.clone();
        let mut api = EarlyApi::new(api_config.token.clone());
        api.diagnostics_port = diagnostics_port;
        api.frontend_identity = frontend_identity;
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
            desktop_ready: false,
            routes_ready: false,
            subsystems: [
                "desk",
                "account",
                "billing",
                "plans",
                "plugins",
                "providers",
                "crons",
                "memory",
                "mesh",
                "neighbors",
                "docs_vectors",
            ]
            .into_iter()
            .map(|name| (name, PhaseState::Starting))
            .collect(),
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
                desktop: None,
                readiness,
            })),
            changes,
            started: Instant::now(),
            token,
            diagnostics_port: codeterm_api::health_routes::NativeDiagnosticsPort(Arc::new(|| None)),
            frontend_identity: codeterm_api::health_routes::NativeFrontendIdentity {
                port: Arc::new(|| None),
                identifier: String::new(),
            },
        }
    }

    fn router(&self) -> Router {
        Router::new().fallback(any(route)).with_state(self.clone())
    }

    pub(super) fn with_startup_events(&self, router: Router) -> Router {
        router.route(
            "/api/startup/events",
            any(startup_events).with_state(StartupEventState {
                changes: self.changes.clone(),
                token: self.token.clone(),
            }),
        )
    }

    pub fn complete(&self, phase: &'static str) {
        let mut gate = self.gate.lock();
        gate.readiness.phases.insert(phase, PhaseState::Ready);
        self.publish(&gate.readiness, phase);
    }

    pub fn complete_subsystem(&self, name: &'static str) {
        self.set_subsystem(name, PhaseState::Ready);
    }

    pub fn fail_subsystem(&self, name: &'static str) {
        self.set_subsystem(name, PhaseState::Failed);
    }

    fn set_subsystem(&self, name: &'static str, state: PhaseState) {
        let mut gate = self.gate.lock();
        gate.readiness.subsystems.insert(name, state);
        self.publish(&gate.readiness, name);
    }

    pub fn publish_desktop(&self, router: Router) -> Result<(), String> {
        let mut gate = self.gate.lock();
        if gate.readiness.state != PhaseState::Starting
            || ["listener", "integrity", "states"]
                .iter()
                .any(|phase| gate.readiness.phases.get(phase) != Some(&PhaseState::Ready))
        {
            return Err("Desktop readiness prerequisites are incomplete".into());
        }
        gate.desktop = Some(router);
        gate.readiness.desktop_ready = true;
        gate.readiness.subsystems.insert("desk", PhaseState::Ready);
        self.publish(&gate.readiness, "desktop");
        Ok(())
    }

    pub fn fail(&self, phase: &'static str) {
        let mut gate = self.gate.lock();
        gate.router = None;
        gate.desktop = None;
        gate.readiness.desktop_ready = false;
        gate.readiness.routes_ready = false;
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
        gate.desktop = None;
        gate.readiness.desktop_ready = true;
        gate.readiness.routes_ready = true;
        gate.readiness.state = PhaseState::Ready;
        self.publish(&gate.readiness, "ready");
        Ok(())
    }
}

#[derive(Clone)]
struct StartupEventState {
    changes: watch::Sender<Readiness>,
    token: Option<String>,
}

fn authenticated(request: &Request, token: Option<&str>) -> bool {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_some_and(|peer| peer.0.ip().is_loopback())
        || token.is_some_and(|token| {
            request
                .headers()
                .get(axum::http::header::AUTHORIZATION)
                .is_some_and(|value| {
                    value.as_bytes().strip_prefix(b"Bearer ") == Some(token.as_bytes())
                })
        })
}

async fn startup_events(State(state): State<StartupEventState>, request: Request) -> Response {
    if !authenticated(&request, state.token.as_deref()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if request.method() != Method::GET {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let mut changes = state.changes.subscribe();
    Sse::new(async_stream::stream! {
        loop {
            let readiness = changes.borrow_and_update().clone();
            let terminal = readiness.state == PhaseState::Failed
                || (readiness.state == PhaseState::Ready && readiness.subsystems.values().all(|state| *state != PhaseState::Starting));
            let event = Event::default().event("startup-state").json_data(readiness);
            match event {
                Ok(event) => yield Ok::<_, Infallible>(event),
                Err(_) => break,
            }
            if terminal || changes.changed().await.is_err() { break; }
        }
    })
    .into_response()
}
async fn route(State(api): State<EarlyApi>, request: Request) -> Response {
    let path = request.uri().path();
    let read = request.method() == Method::GET;
    if path == "/api/startup/events" {
        return startup_events(
            State(StartupEventState {
                changes: api.changes.clone(),
                token: api.token.clone(),
            }),
            request,
        )
        .await;
    }
    let authenticated = authenticated(&request, api.token.as_deref());
    let (router, readiness) = {
        let gate = api.gate.lock();
        let router = gate.router.clone().or_else(|| {
            codeterm_api::startup_routes::serves(path)
                .then(|| gate.desktop.clone())
                .flatten()
        });
        (router, gate.readiness.clone())
    };
    if read && path == "/api/startup" {
        return if authenticated {
            Json(readiness).into_response()
        } else {
            StatusCode::FORBIDDEN.into_response()
        };
    }
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
                "vite_port": api.frontend_identity.vite_port(),
                "cdp_port": api.diagnostics_port.cdp_port(),
                "identifier": api.frontend_identity.identifier(),
            }
        })).into_response();
    }
    (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({
        "ok": false, "error": {"code": if readiness.state == PhaseState::Failed { "startup_failed" } else { "starting" }, "subsystem": subsystem(path), "request_executed": false},
        "startup": readiness,
    }))).into_response()
}

fn subsystem(path: &str) -> &'static str {
    if path.starts_with("/api/account/plan") || path.starts_with("/api/account/entitlements") {
        "billing"
    } else if path.starts_with("/api/account/mesh") || path.starts_with("/api/mesh") {
        "mesh"
    } else if path.starts_with("/api/account/neighbors") || path.starts_with("/api/neighbors") {
        "neighbors"
    } else if path.starts_with("/api/account/") {
        "account"
    } else if path.starts_with("/api/plans") || path.starts_with("/api/teams") {
        "plans"
    } else if path.starts_with("/api/mem") {
        "memory"
    } else if path.starts_with("/api/plugins") || path.starts_with("/api/extensions") {
        "plugins"
    } else if path.starts_with("/api/agents") || path.starts_with("/api/providers") {
        "providers"
    } else if path.starts_with("/api/crons") || path.starts_with("/api/automation") {
        "crons"
    } else {
        "service"
    }
}

#[cfg(test)]
mod tests;
