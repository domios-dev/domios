use axum::{
    extract::Request,
    http::{header, HeaderMap, Method},
    middleware::Next,
    response::Response,
};

fn text(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_owned)
}

pub(super) async fn record(request: Request, next: Next) -> Response {
    let origin = text(request.headers(), "origin");
    let requested_method = text(request.headers(), "access-control-request-method");
    let requested_headers = text(request.headers(), "access-control-request-headers");
    let method = request.method().clone();
    let response = next.run(request).await;
    if let Some(origin) = origin {
        let origin_allowed = response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_some_and(|allowed| allowed == origin.as_str());
        let preflight = method == Method::OPTIONS && requested_method.is_some();
        let methods = text(response.headers(), "access-control-allow-methods");
        let headers = text(response.headers(), "access-control-allow-headers");
        let method_allowed = requested_method.as_ref().is_none_or(|requested| {
            methods.as_ref().is_some_and(|allowed| {
                allowed
                    .split(',')
                    .any(|value| value.trim() == "*" || value.trim() == requested)
            })
        });
        let headers_allowed = requested_headers.as_ref().is_none_or(|requested| {
            headers.as_ref().is_some_and(|allowed| {
                requested.split(',').all(|name| {
                    allowed.split(',').any(|value| {
                        value.trim().eq_ignore_ascii_case(name.trim())
                            || (value.trim() == "*"
                                && !name.trim().eq_ignore_ascii_case("authorization"))
                    })
                })
            })
        });
        if preflight || !origin_allowed {
            log::debug!(
                "[rest-cors] origin={origin:?} method={method} requested_method={requested_method:?} requested_headers={requested_headers:?} origin_allowed={origin_allowed} method_allowed={method_allowed} headers_allowed={headers_allowed} decision={}",
                if origin_allowed && method_allowed && headers_allowed { "allow" } else { "refuse" },
            );
        }
    }
    response
}
