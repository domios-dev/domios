use super::ServiceContext;
use std::time::Duration;
use tokio::sync::oneshot;

type Scope = (String, Option<String>);

static CAPTURES: std::sync::LazyLock<parking_lot::Mutex<std::collections::HashSet<Scope>>> =
    std::sync::LazyLock::new(Default::default);

pub(super) struct ScopeClaim(Scope);

impl ScopeClaim {
    pub(super) fn acquire(workspace: &str, team: Option<&str>) -> Result<Self, String> {
        let scope = (workspace.to_string(), team.map(str::to_string));
        if !CAPTURES.lock().insert(scope.clone()) {
            return Err(failure(
                "close_inventory_busy",
                "A close inventory is being captured for this scope",
            ));
        }
        Ok(Self(scope))
    }
}

impl Drop for ScopeClaim {
    fn drop(&mut self) {
        CAPTURES.lock().remove(&self.0);
    }
}

const CAPTURE_DEADLINE: Duration = Duration::from_secs(10);

pub fn close_inventory_error_data(message: &str) -> Option<serde_json::Value> {
    let value: serde_json::Value = serde_json::from_str(message).ok()?;
    matches!(
        value.get("type")?.as_str()?,
        "close_inventory_timeout"
            | "close_inventory_failed"
            | "close_inventory_busy"
            | "close_scope_busy"
    )
    .then_some(value)
}

pub(super) fn failure(kind: &str, message: &str) -> String {
    serde_json::json!({"type": kind, "message": message}).to_string()
}

#[derive(Debug)]
enum CaptureFailure {
    Timeout,
    WorkerExited,
    Inventory(String),
}

impl CaptureFailure {
    fn message(self) -> String {
        match self {
            Self::Timeout => failure(
                "close_inventory_timeout",
                "Close inventory capture timed out",
            ),
            Self::WorkerExited => failure(
                "close_inventory_failed",
                "Close inventory capture worker exited",
            ),
            Self::Inventory(error) => error,
        }
    }
}

async fn receive<T>(
    receiver: &mut oneshot::Receiver<Result<T, String>>,
    deadline: Duration,
) -> Result<T, CaptureFailure> {
    tokio::time::timeout(deadline, receiver)
        .await
        .map_err(|_| CaptureFailure::Timeout)?
        .map_err(|_| CaptureFailure::WorkerExited)?
        .map_err(CaptureFailure::Inventory)
}

impl ServiceContext {
    pub async fn capture_close_inventory(
        &self,
        workspace: &str,
        team: Option<&str>,
    ) -> Result<Option<String>, String> {
        let service = self.clone();
        let workspace = workspace.to_string();
        let team = team.map(str::to_string);
        let (sender, mut receiver) = oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let result = service.capture_close_inventory_sync(&workspace, team.as_deref());
            let _ = sender.send(result);
        });
        let result = receive(&mut receiver, CAPTURE_DEADLINE).await;
        if matches!(result, Err(CaptureFailure::Timeout)) {
            let service = self.clone();
            tokio::spawn(async move {
                if let Ok(Ok(inventory)) = receiver.await {
                    match tokio::task::spawn_blocking(move || {
                        service.settle_close_inventory(inventory.as_deref())
                    })
                    .await
                    {
                        Ok(Ok(())) => {}
                        result => log::warn!(
                            "[close-inventory] late capture settlement failed: {result:?}"
                        ),
                    }
                }
            });
        }
        result.map_err(CaptureFailure::message)
    }
}

#[cfg(test)]
mod tests;
