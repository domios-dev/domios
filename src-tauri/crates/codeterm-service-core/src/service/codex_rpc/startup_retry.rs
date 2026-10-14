use std::future::Future;

const MAX_ATTEMPTS: usize = 3;
const SUPERSEDED: &str = "Tab launch superseded while sidecar was starting";

pub(super) fn verify_launch(expected: &str, actual: Option<&str>) -> Result<(), String> {
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(SUPERSEDED.into())
    }
}

fn transient(error: &str) -> bool {
    let reason = error
        .split_once(": pid=")
        .map_or(error, |(reason, _)| reason);
    matches!(
        reason,
        "Sidecar listening deadline"
            | "RPC WebSocket readiness deadline"
            | "RPC WebSocket connection failed"
            | "App-server request failed: Deadline"
            | "App-server request failed: Closed(\"RPC reader exited\")"
            | "App-server request failed: Closed(\"RPC writer exited\")"
    )
}

pub(super) async fn run<T, F, Fut>(tab: &str, mut start: F) -> Result<T, String>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        log::info!("[codex-rpc-startup] tab={tab} attempt={attempt}/{MAX_ATTEMPTS} state=starting");
        match start().await {
            Ok(sidecar) => {
                log::info!(
                    "[codex-rpc-startup] tab={tab} attempt={attempt}/{MAX_ATTEMPTS} state=ready"
                );
                return Ok(sidecar);
            }
            Err(error) => {
                let retry = attempt < MAX_ATTEMPTS && transient(&error);
                log::warn!("[codex-rpc-startup] tab={tab} attempt={attempt}/{MAX_ATTEMPTS} retry={retry} reason={error}");
                if !retry {
                    return Err(error);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
