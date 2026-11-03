use crate::{
    runtime,
    viewport::{Viewport, ViewportMode},
    BrowserHost, PageChannel,
};
use serde_json::{json, Value};

fn screenshot_params(mut params: Value, metrics: &Value) -> Result<Value, String> {
    if params.get("clip").is_some() {
        return Ok(params);
    }
    let full = params["captureBeyondViewport"].as_bool().unwrap_or(false);
    let bounds = &metrics[if full {
        "cssContentSize"
    } else {
        "cssLayoutViewport"
    }];
    let dimension = |name: &str| {
        bounds[name]
            .as_f64()
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or_else(|| format!("missing CSS capture {name}"))
    };
    let width = dimension(if full { "width" } else { "clientWidth" })?;
    let height = dimension(if full { "height" } else { "clientHeight" })?;
    let x = bounds[if full { "x" } else { "pageX" }]
        .as_f64()
        .unwrap_or(0.0);
    let y = bounds[if full { "y" } else { "pageY" }]
        .as_f64()
        .unwrap_or(0.0);
    if !x.is_finite() || !y.is_finite() {
        return Err("invalid CSS capture origin".into());
    }
    // An explicit unit-scale clip prevents Chrome from applying the emulated DPR twice.
    params["clip"] = json!({"x":x,"y":y,"width":width,"height":height,"scale":1});
    Ok(params)
}

impl BrowserHost {
    pub async fn capture_screenshot(
        &self,
        page: &PageChannel,
        params: Value,
    ) -> Result<Value, String> {
        let owned = self.viewport.lock().mode() == ViewportMode::Owned;
        let params = if owned && params.get("clip").is_none() {
            let metrics = page
                .send("Page.getLayoutMetrics", json!({}))
                .await
                .map_err(|e| e.to_string())?;
            screenshot_params(params, &metrics)?
        } else {
            params
        };
        page.send("Page.captureScreenshot", params)
            .await
            .map_err(|e| e.to_string())
    }

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
                let mut metrics = runtime
                    .page
                    .send("Page.getLayoutMetrics", json!({}))
                    .await
                    .map_err(|e| e.to_string())?;
                metrics["devicePixelRatio"] = json!(viewport.dpr);
                let shot = runtime
                    .page
                    .send(
                        "Page.captureScreenshot",
                        screenshot_params(
                            json!({"format":"png", "captureBeyondViewport":full}),
                            &metrics,
                        )?,
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

#[cfg(test)]
mod tests;
