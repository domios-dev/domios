use super::BrowserRuntime;
use codeterm_browser::PageChannel;
use serde_json::json;
use tokio::sync::OwnedMutexGuard;

pub(super) struct ActionScope {
    gate: Option<OwnedMutexGuard<()>>,
    page: PageChannel,
    restore_touch: bool,
}

async fn restore(page: &PageChannel) -> Result<(), String> {
    page.send("Emulation.setEmitTouchEventsForMouse", json!({"enabled":true,"configuration":"mobile"}))
        .await.map(|_| ()).map_err(|error| format!(
            "action_unconfirmed: touch emulation restoration failed: {error}; inspect before continuing; never replay"
        ))
}

impl ActionScope {
    pub(super) async fn finish(mut self) -> Result<(), String> {
        if let Some(task) = self.restore_task() {
            return task
                .await
                .map_err(|error| format!("touch restoration task failed: {error}"))?;
        }
        Ok(())
    }

    fn restore_task(&mut self) -> Option<tokio::task::JoinHandle<Result<(), String>>> {
        if !std::mem::take(&mut self.restore_touch) {
            return None;
        }
        let gate = self.gate.take();
        let page = self.page.clone();
        Some(tokio::spawn(async move {
            let _gate = gate;
            let result = restore(&page).await;
            if let Err(error) = &result {
                log::warn!("[browser-input] {error}");
            }
            result
        }))
    }
}

impl Drop for ActionScope {
    fn drop(&mut self) {
        drop(self.restore_task());
    }
}

impl BrowserRuntime {
    pub(super) async fn action_scope(
        &self,
        page: &PageChannel,
        mouse_events: bool,
    ) -> Result<ActionScope, String> {
        let gate = self.action_gate.clone().lock_owned().await;
        let scope = ActionScope {
            gate: Some(gate),
            page: page.clone(),
            restore_touch: mouse_events && self.host.viewport().mobile,
        };
        if scope.restore_touch {
            self.send(
                page,
                "Emulation.setEmitTouchEventsForMouse",
                json!({"enabled":false}),
            )
            .await?;
        }
        Ok(scope)
    }
}
