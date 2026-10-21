use crate::auth::CallerIdentity;
use axum::{extract::rejection::JsonRejection, http::StatusCode, Extension, Json};
use codeterm_types::app_quit::{QuitApiReply, QuitRefusal, QuitSnapshot};
use std::{future::Future, pin::Pin, sync::Arc};

pub type QuitReplyFuture<'a> = Pin<Box<dyn Future<Output = QuitApiReply> + Send + 'a>>;
pub trait AppQuitPort: Send + Sync {
    fn snapshot(&self) -> Result<QuitSnapshot, QuitRefusal>;
    fn request(&self, expected: QuitSnapshot) -> QuitReplyFuture<'_>;
}
#[derive(Clone)]
pub struct AppQuitPortHandle(pub Arc<dyn AppQuitPort>);
pub fn with_port(router: axum::Router, port: AppQuitPortHandle) -> axum::Router {
    router.layer(Extension(port))
}

fn admit(caller: Option<&CallerIdentity>) -> Result<(), QuitRefusal> {
    if matches!(caller, Some(CallerIdentity::Local)) {
        Ok(())
    } else {
        Err(QuitRefusal::IdentityMismatch)
    }
}

pub async fn snapshot(
    caller: Option<Extension<CallerIdentity>>,
    port: Option<Extension<AppQuitPortHandle>>,
) -> Result<Json<QuitSnapshot>, (StatusCode, Json<QuitApiReply>)> {
    admit(caller.as_ref().map(|caller| &caller.0)).map_err(|reason| {
        (
            StatusCode::FORBIDDEN,
            Json(QuitApiReply::Refused { reason }),
        )
    })?;
    let snapshot = port
        .ok_or(QuitRefusal::Unavailable)
        .and_then(|port| port.0 .0.snapshot())
        .and_then(|snapshot| {
            if snapshot.valid() {
                Ok(snapshot)
            } else {
                Err(QuitRefusal::IdentityMismatch)
            }
        });
    snapshot.map(Json).map_err(|reason| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(QuitApiReply::Refused { reason }),
        )
    })
}

pub async fn request(
    caller: Option<Extension<CallerIdentity>>,
    port: Option<Extension<AppQuitPortHandle>>,
    body: Result<Json<QuitSnapshot>, JsonRejection>,
) -> (StatusCode, Json<QuitApiReply>) {
    let caller = caller.as_ref().map(|caller| &caller.0);
    if let Err(reason) = admit(caller) {
        return (
            StatusCode::FORBIDDEN,
            Json(QuitApiReply::Refused { reason }),
        );
    }
    let reply = dispatch(
        caller,
        port.as_ref().map(|port| &port.0),
        body.map(|Json(value)| value).map_err(|_| ()),
    )
    .await;
    (StatusCode::OK, Json(reply))
}

async fn dispatch(
    caller: Option<&CallerIdentity>,
    port: Option<&AppQuitPortHandle>,
    body: Result<QuitSnapshot, ()>,
) -> QuitApiReply {
    if let Err(reason) = admit(caller) {
        return QuitApiReply::Refused { reason };
    }
    let Ok(expected) = body else {
        return QuitApiReply::Refused {
            reason: QuitRefusal::IdentityMismatch,
        };
    };
    if !expected.valid() {
        return QuitApiReply::Refused {
            reason: QuitRefusal::IdentityMismatch,
        };
    }
    match port {
        Some(port) => port.0.request(expected).await,
        None => QuitApiReply::Refused {
            reason: QuitRefusal::Unavailable,
        },
    }
}
#[cfg(test)]
mod tests;
