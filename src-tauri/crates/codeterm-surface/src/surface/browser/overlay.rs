use std::sync::Arc;
use std::time::Duration;

use super::page::PageShared;
use super::BrowserRuntime;
use codeterm_browser::PageChannel;
use serde_json::{json, Value};

const SCRIPT: &str = include_str!("agent-overlay.js");
const API: &str = "window[Symbol.for('domios.browser.presentation')]";
const OVERLAY_RESTORE_DEADLINE: Duration = Duration::from_secs(2);

/// Balances one acked `overlay_capture(true)` with exactly one `overlay_capture(false)` on the
/// original page, even when the owning future is cancelled by a mid-verb JS dialog.
pub(super) struct OverlaySuppressionGuard {
    page: PageChannel,
    shared: Arc<PageShared>,
    active: bool,
}

impl OverlaySuppressionGuard {
    pub(super) async fn restore(mut self, runtime: &BrowserRuntime) -> Result<(), String> {
        self.active = false;
        runtime.overlay_capture(&self.page, false).await
    }
}

impl Drop for OverlaySuppressionGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let page = self.page.clone();
        let shared = self.shared.clone();
        handle.spawn(async move {
            let mut dialogs = shared.subscribe_dialogs();
            let mut closed = page.client().closed();
            let ready = loop {
                if !shared.is_bound_page(&page) {
                    break false;
                }
                if dialogs.borrow_and_update().dialog.is_none() {
                    break true;
                }
                tokio::select! {
                    _ = closed.wait_for(Option::is_some) => break false,
                    changed = dialogs.changed() => {
                        if changed.is_err() {
                            break false;
                        }
                    }
                }
            };
            if !ready || !shared.is_bound_page(&page) {
                return;
            }
            let _ = page
                .send_with_deadline(
                    "Runtime.evaluate",
                    json!({
                        "expression": format!("{API}?.capture(false)"),
                        "returnByValue": true,
                    }),
                    OVERLAY_RESTORE_DEADLINE,
                )
                .await;
        });
    }
}

impl BrowserRuntime {
    pub(super) async fn suppress_overlay(
        &self,
        page: &PageChannel,
    ) -> Result<OverlaySuppressionGuard, String> {
        self.overlay_capture(page, true).await?;
        Ok(OverlaySuppressionGuard {
            page: page.clone(),
            shared: self.shared.clone(),
            active: true,
        })
    }

    pub(super) async fn capture_screenshot(
        &self,
        page: &PageChannel,
        params: Value,
        include_overlay: bool,
    ) -> Result<Value, String> {
        let guard = if !include_overlay {
            Some(self.suppress_overlay(page).await?)
        } else {
            None
        };
        let shot = self.host.capture_screenshot(page, params).await;
        let restored = match guard {
            Some(guard) => guard.restore(self).await,
            None => Ok(()),
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
        let target = serde_json::to_string(page.target_id()).map_err(|error| error.to_string())?;
        let _ = self
            .send(
                page,
                "Runtime.addBinding",
                json!({"name": super::page::CURSOR_BINDING}),
            )
            .await;
        let script = format!("Object.defineProperty(globalThis, Symbol.for('domios.browser.target'), {{value:{target}, configurable:true}});{SCRIPT}");
        self.send(
            page,
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source":script}),
        )
        .await?;
        self.evaluate(page, &script, false).await?;
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
