// Axum converts handler results on its runtime worker; large JSON must be encoded off that worker.
use axum::{
    extract::{FromRequest, FromRequestParts, Request},
    handler::Handler,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::{future::Future, pin::Pin};

#[derive(Clone)]
pub(crate) struct BlockingJson<H>(H);

pub(crate) fn handler<H>(handler: H) -> BlockingJson<H> {
    BlockingJson(handler)
}

pub(crate) async fn respond<R: IntoResponse + Send + 'static>(reply: R) -> Response {
    match prepare((), move || Ok::<_, std::convert::Infallible>(reply)).await {
        Ok(Ok(response)) => response,
        Ok(Err(never)) => match never {},
        Err(error) => {
            log::debug!("[json-response] encoding worker failed: {error}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub(crate) async fn prepare<R: IntoResponse, E: Send + 'static, G: Send + 'static>(
    guard: G,
    prepare: impl FnOnce() -> Result<R, E> + Send + 'static,
) -> Result<Result<Response, E>, tokio::task::JoinError> {
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        prepare().map(IntoResponse::into_response)
    })
    .await
}

impl<H, F, R, S> Handler<((),), S> for BlockingJson<H>
where
    H: FnOnce() -> F + Clone + Send + Sync + 'static,
    F: Future<Output = R> + Send,
    R: IntoResponse + Send + 'static,
    S: Send + Sync + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;
    fn call(self, _request: Request, _state: S) -> Self::Future {
        Box::pin(async move { respond((self.0)().await).await })
    }
}

macro_rules! impl_handler {
    ([$($part:ident),*], $last:ident) => {
        #[allow(non_snake_case)]
        impl<H, F, R, S, M, $($part,)* $last> Handler<(M, $($part,)* $last,), S> for BlockingJson<H>
        where
            H: FnOnce($($part,)* $last) -> F + Clone + Send + Sync + 'static,
            F: Future<Output = R> + Send,
            R: IntoResponse + Send + 'static,
            S: Send + Sync + 'static,
            $($part: FromRequestParts<S> + Send,)*
            $last: FromRequest<S, M> + Send,
        {
            type Future = Pin<Box<dyn Future<Output = Response> + Send>>;
            fn call(self, request: Request, state: S) -> Self::Future {
                Box::pin(async move {
                    #[allow(unused_mut)]
                    let (mut parts, body) = request.into_parts();
                    $(let $part = match $part::from_request_parts(&mut parts, &state).await {
                        Ok(value) => value,
                        Err(rejection) => return rejection.into_response(),
                    };)*
                    let request = Request::from_parts(parts, body);
                    let $last = match $last::from_request(request, &state).await {
                        Ok(value) => value,
                        Err(rejection) => return rejection.into_response(),
                    };
                    respond((self.0)($($part,)* $last).await).await
                })
            }
        }
    };
}
impl_handler!([], A);
impl_handler!([A], B);
impl_handler!([A, B], C);
impl_handler!([A, B, C], D);
impl_handler!([A, B, C, D], E);
impl_handler!([A, B, C, D, E], G);
impl_handler!([A, B, C, D, E, G], I);
impl_handler!([A, B, C, D, E, G, I], J);
