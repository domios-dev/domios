//! Post-update message injection.
//!
//! When `update --apply --after-update TEXT` runs, the backend writes the
//! text to a file before triggering the Tauri restart. On next boot this
//! module picks it up, waits for the General Agent tab to be ready, sends
//! the text through `send` semantics (as if the user typed it), and
//! deletes the file.

use std::path::PathBuf;
use std::time::Duration;

use crate::service::ServiceState;

const FILENAME: &str = "post-update-message.txt";

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

    // takes a few seconds to come up after restart.
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    let mut tab_id: Option<String> = None;
    while std::time::Instant::now() < deadline {
        let candidate = {
            let mux = svc.mux.write();
            mux.resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)
        };
        if let Some(id) = candidate {
            let has_pty = svc
                .mux
                .write()
                .get_tab(&id)
                .and_then(|p| p.pty_id.clone())
                .is_some();
            if has_pty {
                tab_id = Some(id);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let Some(tab_id) = tab_id else {
        log::warn!("[post-update] general Tab never became ready — leaving message file in place");
        return Ok(());
    };

    // Grace for the agent to finish drawing its prompt and accept input.
    tokio::time::sleep(Duration::from_secs(5)).await;

    match svc.send_text(&tab_id, &text, true) {
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
