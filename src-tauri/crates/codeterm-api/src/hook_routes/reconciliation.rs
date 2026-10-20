use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;
use std::task::{Context, Poll};

use axum::body::{Body, BodyDataStream, Bytes};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::Stream;
use parking_lot::Mutex;
use tokio::sync::oneshot;

static QUEUES: LazyLock<Mutex<HashMap<String, (String, oneshot::Receiver<()>)>>> =
    LazyLock::new(Mutex::default);

struct Work {
    key: String,
    id: String,
    previous: Option<oneshot::Receiver<()>>,
    finished: oneshot::Sender<()>,
    run: Pin<Box<dyn Future<Output = ()> + Send>>,
}

struct WorkStream {
    body: BodyDataStream,
    work: Option<Work>,
}

impl Stream for WorkStream {
    type Item = Result<Bytes, axum::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let result = Pin::new(&mut self.body).poll_next(cx);
        if matches!(result, Poll::Ready(None)) {
            self.schedule();
        }
        result
    }
}

impl WorkStream {
    fn schedule(&mut self) {
        if let Some(work) = self.work.take() {
            tokio::spawn(async move {
                if let Some(previous) = work.previous {
                    let _ = previous.await;
                }
                work.run.await;
                let mut queues = QUEUES.lock();
                if queues.get(&work.key).is_some_and(|(id, _)| id == &work.id) {
                    queues.remove(&work.key);
                }
                let _ = work.finished.send(());
            });
        }
    }
}

impl Drop for WorkStream {
    fn drop(&mut self) {
        self.schedule();
    }
}

fn after_response_work(
    response: Json<serde_json::Value>,
    key: String,
    run: impl Future<Output = ()> + Send + 'static,
) -> Response {
    let (finished, receiver) = oneshot::channel();
    let id = uuid::Uuid::new_v4().to_string();
    let previous = QUEUES.lock().insert(key.clone(), (id.clone(), receiver));
    let response = response.into_response();
    let (parts, body) = response.into_parts();
    Response::from_parts(
        parts,
        Body::from_stream(WorkStream {
            body: body.into_data_stream(),
            work: Some(Work {
                key,
                id,
                previous: previous.map(|(_, receiver)| receiver),
                finished,
                run: Box::pin(run),
            }),
        }),
    )
}

pub(super) fn after_response(
    response: Json<serde_json::Value>,
    svc: codeterm_service_core::service::ServiceState,
    tab_id: String,
    route: &'static str,
) -> Response {
    let binding = svc.mux.read().get_tab(&tab_id).map(|tab| {
        (
            tab.agent_type.clone(),
            tab.agent_session_id.clone(),
            tab.pty_id.clone(),
            tab.session_binding_nonce.clone(),
            tab.reported_session_binding_nonce.clone(),
        )
    });
    after_response_work(response, tab_id.clone(), async move {
        let owner = svc.clone();
        let target = tab_id.clone();
        let current = tokio::task::spawn_blocking(move || {
            let current = owner.mux.read().get_tab(&target).map(|tab| {
                (
                    tab.agent_type.clone(),
                    tab.agent_session_id.clone(),
                    tab.pty_id.clone(),
                    tab.session_binding_nonce.clone(),
                    tab.reported_session_binding_nonce.clone(),
                )
            });
            current
        })
        .await
        .ok()
        .flatten();
        if binding.is_none() || current != binding {
            log::info!("[hook/timing] tab={tab_id} route={route} phase=reconciliation_skipped reason=binding_changed");
            return;
        }
        let started = std::time::Instant::now();
        log::info!("[hook/timing] tab={tab_id} route={route} phase=reconciliation_started");
        svc.reconcile_chat_send_hook(&tab_id).await;
        log::info!(
            "[hook/timing] tab={tab_id} route={route} phase=reconciliation_completed elapsed_ms={}",
            started.elapsed().as_millis()
        );
    })
}

#[cfg(test)]
mod tests;
