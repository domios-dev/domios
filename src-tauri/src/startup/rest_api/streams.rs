use axum::{
    body::Body,
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};
use futures_util::StreamExt;
use tokio::sync::watch;

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
        router.layer(axum::middleware::from_fn(
            move |request: Request, next: Next| {
                let closed = closed.clone();
                async move {
                    if *closed.borrow() {
                        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    let response = next.run(request).await;
                    if response
                        .headers()
                        .get(axum::http::header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .is_some_and(|value| value.split(';').next() == Some("text/event-stream"))
                    {
                        let (parts, body) = response.into_parts();
                        Response::from_parts(
                            parts,
                            Body::from_stream(until_closed(body.into_data_stream(), closed)),
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
