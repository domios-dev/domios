use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use std::convert::Infallible;
use tower::{Service, ServiceExt};

pub(super) async fn serve_or_missing<S>(service: S, request: Request<Body>) -> Response
where
    S: Service<Request<Body>, Error = Infallible> + Send,
    S::Response: IntoResponse,
    S::Future: Send,
{
    match service.oneshot(request).await {
        Ok(response) => {
            let response = response.into_response();
            if response.status() == StatusCode::NOT_FOUND {
                missing_response()
            } else {
                response
            }
        }
        Err(never) => match never {},
    }
}

fn missing_response() -> Response {
    let mut response = (
        StatusCode::NOT_FOUND,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("../frontend_missing.html"),
    )
        .into_response();
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-cache"),
    );
    response
}
