use super::*;

pub(super) async fn wake(
    State(state): State<AppState>,
    Extension(caller): Extension<CallerIdentity>,
    Path(raw): Path<String>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, &'static str)> {
    local_target(&caller, &raw)?;
    drop(enqueue(move || {
        if let Err(reason) = state.svc.wake_tab_on_request(&raw) {
            log::warn!("[tab-wake] tab={raw}: {reason}");
        }
    }));
    Ok((StatusCode::ACCEPTED, Json(json!({ "accepted": true }))))
}

fn enqueue(work: impl FnOnce() + Send + 'static) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(work)
}
