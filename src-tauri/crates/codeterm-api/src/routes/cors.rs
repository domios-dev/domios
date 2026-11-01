use tower_http::cors::{AllowOrigin, CorsLayer};

pub(super) const FRONTEND_REQUEST_HEADERS: [&str; 11] = [
    "authorization",
    "if-none-match",
    "content-type",
    "accept",
    "x-codeterm-tab-id",
    "x-codeterm-client-id",
    "x-codeterm-session-id",
    "x-codeterm-binding-nonce",
    "x-codeterm-session-binding-nonce",
    "x-codeterm-plan-confirmation",
    "x-codeterm-blueprint-confirmation",
];

pub(super) fn configured_origin(origin: &str) -> Option<axum::http::HeaderValue> {
    if !codeterm_service_core::config::exact_cors_origin(origin) {
        return None;
    }
    origin.parse().ok()
}

pub fn cors_layer(frontend_origins: &[String], configured_origins: &[String]) -> CorsLayer {
    let mut origins = frontend_origins
        .iter()
        .filter_map(|origin| origin.parse::<axum::http::HeaderValue>().ok())
        .collect::<Vec<_>>();
    for (index, origin) in configured_origins.iter().enumerate() {
        if let Some(value) = configured_origin(origin) {
            if !origins.contains(&value) {
                origins.push(value);
            }
        } else {
            log::warn!(
                "[rest-cors] refused api.cors_allow_origins entry={index}: expected an exact HTTP(S) origin"
            );
        }
    }
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers(FRONTEND_REQUEST_HEADERS.map(axum::http::HeaderName::from_static))
        .expose_headers([
            axum::http::header::CONTENT_SECURITY_POLICY,
            axum::http::HeaderName::from_static("x-domios-plugin-media-frame"),
            axum::http::HeaderName::from_static("x-domios-plugin-view-frame"),
            axum::http::HeaderName::from_static("x-domios-plugin-view-document"),
            axum::http::HeaderName::from_static("x-domios-plugin-view-registration"),
            axum::http::header::ETAG,
        ])
}
