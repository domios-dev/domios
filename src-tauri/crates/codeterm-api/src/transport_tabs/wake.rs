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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn wake_queue_returns_before_materialization_finishes() {
        let (release, released) = std::sync::mpsc::channel();
        let (started, ready) = tokio::sync::oneshot::channel();
        let task = enqueue(move || {
            let _ = started.send(());
            let _ = released.recv();
        });
        ready.await.unwrap();
        assert!(!task.is_finished());
        release.send(()).unwrap();
        task.await.unwrap();
    }
}
