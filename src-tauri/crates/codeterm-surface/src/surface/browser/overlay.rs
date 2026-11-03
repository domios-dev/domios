use super::BrowserRuntime;
use codeterm_browser::PageChannel;
use serde_json::{json, Value};
use std::time::Duration;

const SCRIPT: &str = include_str!("agent-overlay.js");
const API: &str = "window[Symbol.for('domios.browser.presentation')]";

impl BrowserRuntime {
    pub(super) async fn capture_screenshot(
        &self,
        page: &PageChannel,
        params: Value,
        include_overlay: bool,
    ) -> Result<Value, String> {
        if !include_overlay {
            self.overlay_capture(page, true).await?;
        }
        let shot = page
            .send("Page.captureScreenshot", params)
            .await
            .map_err(super::cdp_error);
        let restored = if !include_overlay {
            self.overlay_capture(page, false).await
        } else {
            Ok(())
        };
        let shot = shot?;
        restored?;
        Ok(shot)
    }

    pub(super) async fn install_overlay(&self, page: &PageChannel) -> Result<(), String> {
        let mut installed = self.overlay_session.lock().await;
        if installed.as_deref() == Some(page.session_id()) {
            return Ok(());
        }
        self.send(
            page,
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source":SCRIPT}),
        )
        .await?;
        self.evaluate(page, SCRIPT, false).await?;
        *installed = Some(page.session_id().to_string());
        Ok(())
    }

    pub(super) async fn present(&self, page: &PageChannel, request: Value) {
        self.present_on(page, None, request).await;
    }

    pub(super) async fn present_on(
        &self,
        page: &PageChannel,
        object_id: Option<&str>,
        mut request: Value,
    ) {
        self.shared.show_cursor(page);
        // The cursor's name tag shows the agent driving the action: the actor when another agent drives, else the owner.
        if let Some(fields) = request.as_object_mut() {
            let state = self.shared.state();
            let name = if state.actor.is_empty() {
                state.owner
            } else {
                state.actor
            };
            fields.entry("name").or_insert(Value::String(name));
        }
        let function = format!(
            "function(request) {{ return {API}?.present(request, this instanceof Element ? this : document.activeElement); }}"
        );
        let command = if let Some(id) = object_id {
            json!({"objectId":id,"functionDeclaration":function,"arguments":[{"value":request}],"awaitPromise":true,"returnByValue":true})
        } else {
            let expression = format!("{API}?.present({}, document.activeElement)", request);
            json!({"expression":expression,"awaitPromise":true,"returnByValue":true})
        };
        let method = if object_id.is_some() {
            "Runtime.callFunctionOn"
        } else {
            "Runtime.evaluate"
        };
        if !matches!(
            tokio::time::timeout(Duration::from_millis(600), self.send(page, method, command))
                .await,
            Ok(Ok(_))
        ) {
            let _ = tokio::time::timeout(Duration::from_millis(100), self.hide_agent_cursor(page))
                .await;
        }
    }

    pub(super) async fn hide_agent_cursor(&self, page: &PageChannel) {
        if !self.shared.take_cursor(page) {
            return;
        }
        let _ = self.evaluate(page, &format!("{API}?.hide()"), false).await;
    }

    pub(super) fn hide_cursor_for_viewer(&self, page: &PageChannel) {
        if !self.shared.take_cursor(page) {
            return;
        }
        let page = page.clone();
        tokio::spawn(async move {
            let _ = page
                .send(
                    "Runtime.evaluate",
                    json!({
                        "expression": format!("{API}?.hide()"), "returnByValue": true
                    }),
                )
                .await;
        });
    }

    pub(super) async fn overlay_capture(
        &self,
        page: &PageChannel,
        hidden: bool,
    ) -> Result<(), String> {
        self.evaluate(page, &format!("{API}?.capture({hidden})"), false)
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests;
