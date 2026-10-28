use crate::{
    runtime,
    viewport::{Viewport, ViewportMode},
    BrowserHost,
};
use serde_json::{json, Value};

impl BrowserHost {
    pub async fn capture_with_viewport(
        &self,
        viewport: Viewport,
        full: bool,
    ) -> Result<Value, String> {
        let lifecycle = self.lifecycle.clone().lock_owned().await;
        if self.viewport.lock().mode() != ViewportMode::Owned {
            return Err("refused_attached: device metrics require an owned target".into());
        }
        let runtime = self.current().ok_or("browser is closed")?;
        let previous = self.viewport();
        let resizes = self.resizes.clone();
        let hold = self.frames.hold_for_capture();
        tokio::spawn(async move {
            let _lifecycle = lifecycle;
            let _resizes = resizes.lock().await;
            let _hold = hold;
            let result = async {
                runtime::apply_viewport(
                    &runtime.client,
                    &runtime.page,
                    runtime.window_id,
                    viewport,
                )
                .await?;
                let metrics = runtime
                    .page
                    .send("Page.getLayoutMetrics", json!({}))
                    .await
                    .map_err(|e| e.to_string())?;
                let shot = runtime
                    .page
                    .send(
                        "Page.captureScreenshot",
                        json!({"format":"png", "captureBeyondViewport":full}),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(json!({"data": shot.get("data"), "metrics": metrics}))
            }
            .await;
            let restored = runtime::apply_viewport(
                &runtime.client,
                &runtime.page,
                runtime.window_id,
                previous,
            )
            .await;
            match (result, restored) {
                (_, Err(error)) => Err(format!("restore_failed: {error}")),
                (result, Ok(())) => result,
            }
        })
        .await
        .map_err(|e| format!("capture task failed: {e}"))?
    }
}
