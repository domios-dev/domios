use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
};

pub(super) async fn record(request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str())
        .unwrap_or("<unmatched>")
        .to_owned();
    codeterm_service_core::crash::request_activity::track(&method, &route, next.run(request)).await
}

#[cfg(test)]
mod tests;
