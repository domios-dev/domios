use axum::{
    body::Body,
    extract::{ConnectInfo, Request},
    http::{HeaderMap, Method, Uri},
    response::Response,
    Extension, Router,
};
use std::net::SocketAddr;
use tower::ServiceExt;

#[derive(Clone)]
pub(crate) struct Dispatcher {
    router: Router,
    service: codeterm_service_core::service::ServiceState,
}

pub(crate) fn install(
    router: Router,
    service: codeterm_service_core::service::ServiceState,
) -> Router {
    let dispatcher = Dispatcher::new(router.clone(), service);
    router.layer(Extension(dispatcher))
}

#[derive(Clone, Default)]
pub(crate) struct Context {
    peer: Option<ConnectInfo<SocketAddr>>,
    tailnet: Option<codeterm_service_core::service::account_mesh::tailnet::TailnetListener>,
}

impl Context {
    pub fn from_request(request: &Request) -> Self {
        Self {
            peer: request.extensions().get().copied(),
            tailnet: request.extensions().get().copied(),
        }
    }
}

pub(crate) fn request(
    method: Method,
    uri: Uri,
    mut headers: HeaderMap,
    context: Context,
    body: Body,
) -> Request {
    let nominated = headers
        .get("connection")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_owned())
        .collect::<Vec<_>>();
    for key in nominated {
        headers.remove(&key);
    }
    for key in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "content-length",
        "accept-encoding",
    ] {
        headers.remove(key);
    }
    let metadata = headers
        .keys()
        .filter(|key| key.as_str().starts_with("x-domios-mux-"))
        .cloned()
        .collect::<Vec<_>>();
    for key in metadata {
        headers.remove(key);
    }
    headers.insert(
        "x-domios-mux-request",
        axum::http::HeaderValue::from_static("1"),
    );
    let mut request = Request::new(body);
    *request.method_mut() = method;
    *request.uri_mut() = uri;
    *request.headers_mut() = headers;
    if let Some(peer) = context.peer {
        request.extensions_mut().insert(peer);
    }
    if let Some(tailnet) = context.tailnet {
        request.extensions_mut().insert(tailnet);
    }
    request
}

impl Dispatcher {
    pub fn new(router: Router, service: codeterm_service_core::service::ServiceState) -> Self {
        Self { router, service }
    }
    pub async fn call(&self, request: Request) -> Response {
        let router = self
            .service
            .try_ext::<codeterm_service_core::ports::rest::LocalApiRouter>()
            .map(|published| published.0.clone())
            .unwrap_or_else(|| self.router.clone());
        #[cfg(debug_assertions)]
        let result = {
            let method = request.method().to_string();
            let route = request.uri().path().to_owned();
            let _timing = codeterm_service_core::startup::runtime_diagnostics::RequestTiming::start(
                &method, &route,
            );
            codeterm_service_core::startup::runtime_diagnostics::observe_polls(
                &method,
                &route,
                router.oneshot(request),
            )
            .await
        };
        #[cfg(not(debug_assertions))]
        let result = router.oneshot(request).await;
        match result {
            Ok(response) => response,
            Err(never) => match never {},
        }
    }
}
