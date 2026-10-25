use axum::{body::Body, http::StatusCode, response::IntoResponse, response::Response};
use axum::{
    extract::{ConnectInfo, Request, State},
    middleware::Next,
};
use codeterm_service_core::MeshStateHandle;
use futures_util::StreamExt;
use std::net::SocketAddr;

use crate::routes::AppState;

pub(crate) async fn middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    if req
        .extensions()
        .get::<codeterm_service_core::service::account_mesh::tailnet::TailnetListener>()
        .is_some()
    {
        return next.run(req).await;
    }
    if req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_none_or(|peer| peer.0.ip().to_canonical().is_loopback())
    {
        return next.run(req).await;
    }
    let Some(fence) = RemoteFence::admit(state.mesh) else {
        return refused();
    };
    let response = tokio::select! {
        biased;
        _ = fence.closed() => return refused(),
        response = next.run(req) => response,
    };
    fence.fence_response(response)
}

#[derive(Clone)]
pub(super) struct RemoteFence {
    mesh: MeshStateHandle,
    epoch: u64,
}

impl RemoteFence {
    pub(super) fn admit(mesh: MeshStateHandle) -> Option<Self> {
        let epoch = mesh.lock().remote_access_epoch()?;
        Some(Self { mesh, epoch })
    }

    pub(super) async fn closed(&self) {
        loop {
            let mut changed = {
                let mesh = self.mesh.lock();
                if mesh.remote_access_epoch() != Some(self.epoch) {
                    return;
                }
                mesh.gate_watch()
            };
            if changed.changed().await.is_err() {
                return;
            }
        }
    }

    pub(super) fn fence_response(self, response: Response) -> Response {
        let (parts, body) = response.into_parts();
        let closed = Box::pin(async move { self.closed().await });
        Response::from_parts(
            parts,
            Body::from_stream(body.into_data_stream().take_until(closed)),
        )
    }
}

pub(super) fn refused() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Remote access is closed while signed out",
    )
        .into_response()
}
