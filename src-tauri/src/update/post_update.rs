//! Post-update message injection.
//!
//! When `update --apply --after-update TEXT` runs, the backend writes the
//! text to a file before triggering the Tauri restart. On next boot this
//! module picks it up, waits for the General Agent tab to declare itself
//! ready, sends the text through `send` semantics (as if the user typed it),
//! and deletes the file.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::service::boot_stage::BootStage;
use crate::service::ServiceState;

const FILENAME: &str = "post-update-message.txt";
const READY_DEADLINE: Duration = Duration::from_secs(60);

fn path() -> PathBuf {
    crate::paths::data_dir().join(FILENAME)
}

pub fn stash(text: &str) -> std::io::Result<()> {
    let p = path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, text)
}

pub fn spawn(svc: ServiceState) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run(&svc).await {
            log::warn!("[post-update] pickup failed: {}", e);
        }
    });
}

async fn run(svc: &ServiceState) -> Result<(), String> {
    let p = path();
    let text = match std::fs::read_to_string(&p) {
        Ok(t) if !t.trim().is_empty() => t,
        _ => return Ok(()),
    };
    log::info!(
        "[post-update] message queued ({} chars) — waiting for general Tab",
        text.len()
    );

    let mux = svc.mux.clone();
    let ready = tokio::task::spawn_blocking(move || await_ready_general_tab(&mux, READY_DEADLINE))
        .await
        .map_err(|e| format!("readiness wait task failed: {e}"))?;

    let Some(tab_id) = ready else {
        log::warn!("[post-update] general Tab never became ready — leaving message file in place");
        return Ok(());
    };

    match svc.send_text(&tab_id, &text) {
        Ok(_) => {
            log::info!("[post-update] injected message into general Tab {}", tab_id);
            if let Err(e) = std::fs::remove_file(&p) {
                log::warn!("[post-update] failed to delete {}: {}", p.display(), e);
            }
        }
        Err(e) => {
            log::warn!(
                "[post-update] send_text failed: {} — leaving file for retry on next boot",
                e
            );
        }
    }
    Ok(())
}

fn await_ready_general_tab(mux: &crate::MuxState, deadline: Duration) -> Option<String> {
    let mut ready = None;
    crate::service::boot_wait::wait_until(Instant::now() + deadline, || {
        ready = ready_general_tab(mux);
        ready.is_some()
    });
    ready
}

fn ready_general_tab(mux: &crate::MuxState) -> Option<String> {
    let mux = mux.read();
    let tab_id = mux.resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)?;
    let tab = mux.get_tab(&tab_id)?;
    accepts_injected_message(tab.pty_id.as_deref(), tab.boot_stage).then_some(tab_id)
}

fn accepts_injected_message(pty_id: Option<&str>, stage: Option<BootStage>) -> bool {
    pty_id.is_some() && stage == Some(BootStage::AgentReady)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injection_waits_for_the_declared_ready_stage() {
        assert!(accepts_injected_message(
            Some("pty-ga"),
            Some(BootStage::AgentReady)
        ));
    }

    #[test]
    fn a_live_pty_short_of_ready_is_not_injectable() {
        assert!(!accepts_injected_message(Some("pty-ga"), None));
        assert!(!accepts_injected_message(
            Some("pty-ga"),
            Some(BootStage::SpawnFailed)
        ));
        assert!(!accepts_injected_message(None, Some(BootStage::AgentReady)));
    }
}
