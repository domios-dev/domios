use super::BrowserRuntime;
use codeterm_browser::{EventStream, PageChannel};
use serde_json::{json, Value};

fn drag_data(data: &Value) -> Result<Value, String> {
    let items = data["items"]
        .as_array()
        .ok_or("missing intercepted drag items")?;
    if items.len() > 64
        || data.to_string().len() > 1_048_576
        || data["files"]
            .as_array()
            .is_some_and(|files| !files.is_empty())
    {
        return Err("unsupported intercepted drag payload".into());
    }
    if data["dragOperationsMask"].as_u64().is_none() {
        return Err("missing drag operations mask".into());
    }
    Ok(data.clone())
}

impl BrowserRuntime {
    async fn drag_observation(
        &self,
        page: &PageChannel,
        operation: &str,
        token: &str,
    ) -> Result<Value, String> {
        self.evaluate(
            page,
            &format!(
                "{}({},{})",
                include_str!("drag-observer.js"),
                json!(operation),
                json!(token)
            ),
            false,
        )
        .await
    }

    pub(super) async fn drag_action(
        &self,
        page: &PageChannel,
        points: [f64; 4],
        epoch: Value,
    ) -> Result<String, String> {
        let token = uuid::Uuid::new_v4().to_string();
        let mut intercepted = page.subscribe_session(&["Input.dragIntercepted"]);
        if self.drag_observation(page, "arm", &token).await? != Value::Bool(true) {
            return Err("stale: another drag observation is active".into());
        }
        let enabled = self
            .send(page, "Input.setInterceptDrags", json!({"enabled":true}))
            .await;
        if let Err(error) = enabled {
            let _ = self.drag_observation(page, "clear", &token).await;
            return Err(error);
        }
        let outcome = self
            .complete_action(page, false, async {
                if self
                    .evaluate(
                        page,
                        &format!("{}({})", include_str!("pointer-check.js"), epoch),
                        false,
                    )
                    .await?
                    != Value::Bool(true)
                {
                    return Err("stale: page changed while preparing drag".into());
                }
                self.dispatch_drag(page, points, &token, &mut intercepted)
                    .await
            })
            .await;
        if outcome.is_err() {
            let _ = self.send(page, "Input.cancelDragging", json!({})).await;
        }
        let reset = self
            .send(page, "Input.setInterceptDrags", json!({"enabled":false}))
            .await;
        let clear = self.drag_observation(page, "clear", &token).await;
        self.shared.invalidate_snapshot();
        let (evidence, completion) = outcome?;
        reset.map_err(|_| {
            "action_unconfirmed: drag interception cleanup failed; inspect before continuing"
        })?;
        clear.map_err(|_| {
            "action_unconfirmed: drag observer cleanup failed; inspect before continuing"
        })?;
        Ok(format!(
            "drag: {evidence}; {completion}; re-snapshot to verify the resulting state"
        ))
    }

    async fn drag_mouse(
        &self,
        page: &PageChannel,
        kind: &str,
        x: f64,
        y: f64,
        buttons: i64,
    ) -> Result<(), String> {
        self.send(page,"Input.dispatchMouseEvent",json!({"type":kind,"x":x,"y":y,"button":if buttons == 0 && kind == "mouseMoved" {"none"} else {"left"},"buttons":buttons,"clickCount":1})).await.map(|_| ())
    }

    async fn dispatch_drag(
        &self,
        page: &PageChannel,
        [x, y, end_x, end_y]: [f64; 4],
        token: &str,
        intercepted: &mut EventStream,
    ) -> Result<Value, String> {
        self.drag_mouse(page, "mouseMoved", x, y, 0).await?;
        self.drag_mouse(page, "mousePressed", x, y, 1).await?;
        self.drag_mouse(page, "mouseMoved", (x + end_x) / 2.0, (y + end_y) / 2.0, 1)
            .await?;
        self.drag_mouse(page, "mouseMoved", end_x, end_y, 1).await?;
        let state = self.drag_observation(page, "read", token).await?;
        if state.is_null() || intercepted.has_lost_events() || intercepted.is_closed() {
            return Err("drag observation lost".into());
        }
        let pending = intercepted.try_recv().ok();
        if state["started"] == true || pending.is_some() {
            let event = match pending {
                Some(event) => event,
                None => intercepted
                    .recv()
                    .await
                    .ok_or("drag interception observation closed")?,
            };
            let data = drag_data(&event.params["data"])?;
            for kind in ["dragEnter", "dragOver", "drop"] {
                self.send(
                    page,
                    "Input.dispatchDragEvent",
                    json!({"type":kind,"x":end_x,"y":end_y,"data":data}),
                )
                .await?;
            }
            if self.drag_observation(page, "read", token).await?["dropped"] != true {
                return Err("native drop was not observed; inspect before continuing".into());
            }
            self.drag_mouse(page, "mouseReleased", end_x, end_y, 0)
                .await?;
            Ok(json!("native drop event observed"))
        } else {
            self.drag_mouse(page, "mouseReleased", end_x, end_y, 0)
                .await?;
            let state = self.drag_observation(page, "read", token).await?;
            if state["started"] == true || state["released"] != true {
                return Err("pointer release was not observed; inspect before continuing".into());
            }
            Ok(json!("pointer release event observed"))
        }
    }
}

#[cfg(test)]
mod tests;
