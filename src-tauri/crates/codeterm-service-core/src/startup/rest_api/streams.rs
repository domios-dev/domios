use axum::{
    body::Body,
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};
use futures_util::StreamExt;
use tokio::sync::watch;

mod diagnostics;

#[derive(Clone)]
pub(super) struct GenerationStreams(watch::Sender<bool>);

impl GenerationStreams {
    pub(super) fn new() -> Self {
        Self(watch::channel(false).0)
    }

    pub(super) fn close(&self) {
        self.0.send_replace(true);
    }

    pub(super) fn router(&self, router: Router) -> Router {
        let closed = self.0.subscribe();
        let diagnostics = diagnostics::RequestStreams::default();
        router.layer(axum::middleware::from_fn(
            move |request: Request, next: Next| {
                let closed = closed.clone();
                let diagnostics = diagnostics.clone();
                async move {
                    if *closed.borrow() {
                        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    #[cfg(debug_assertions)]
                    let _timing = crate::startup::runtime_diagnostics::RequestTiming::start(
                        request.method().as_str(),
                        request
                            .extensions()
                            .get::<axum::extract::MatchedPath>()
                            .map_or(request.uri().path(), |path| path.as_str()),
                    );
                    let context = diagnostics.accept(&request);
                    let response = next.run(request).await;
                    if response
                        .headers()
                        .get(axum::http::header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .is_some_and(|value| value.split(';').next() == Some("text/event-stream"))
                    {
                        let guard = diagnostics.open(context);
                        let (parts, body) = response.into_parts();
                        Response::from_parts(
                            parts,
                            Body::from_stream(diagnostics::track(
                                until_closed(body.into_data_stream(), closed),
                                guard,
                            )),
                        )
                    } else {
                        response
                    }
                }
            },
        ))
    }
}

fn until_closed<S: futures_core::Stream>(
    stream: S,
    mut closed: watch::Receiver<bool>,
) -> impl futures_core::Stream<Item = S::Item> {
    async_stream::stream! {
        tokio::pin!(stream);
        loop {
            if *closed.borrow_and_update() { return; }
            tokio::select! {
                biased;
                change = closed.changed() => { if change.is_err() { return; } },
                item = stream.next() => match item { Some(item) => yield item, None => return },
            }
        }
    }
}

#[cfg(test)]
mod tests;
