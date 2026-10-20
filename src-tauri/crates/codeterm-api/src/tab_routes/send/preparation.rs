use axum::http::StatusCode;
use codeterm_service_core::service::ServiceState;
use std::path::PathBuf;

use super::{pad_write_refusal, SendRejection};

pub(super) async fn stage(
    svc: &ServiceState,
    tab_id: &str,
    text: String,
    enabled: bool,
) -> Result<String, SendRejection> {
    if !enabled {
        return Ok(text);
    }
    let svc = svc.clone();
    let tab_id = tab_id.to_owned();
    tokio::task::spawn_blocking(move || svc.run_outbound_stages(&tab_id, text))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "send preparation task failed",
            )
                .into()
        })
}

pub(super) async fn substitute(
    svc: &ServiceState,
    tab_id: &str,
    text: String,
    delivered_bytes: usize,
) -> Result<(String, Option<PathBuf>), SendRejection> {
    let svc = svc.clone();
    let target = tab_id.to_owned();
    tokio::task::spawn_blocking(move || {
        svc.substitute_paste_limited_body(&target, text, delivered_bytes)
    })
    .await
    .map_err(|_| {
        SendRejection::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            "send preparation task failed",
        ))
    })?
    .map_err(|detail| pad_write_refusal(tab_id, detail))
}
