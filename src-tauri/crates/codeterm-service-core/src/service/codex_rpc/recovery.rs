use crate::service::ServiceContext;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

struct Recovery {
    nonce: String,
    id: String,
    result: Option<bool>,
}

static REATTACHING: LazyLock<Mutex<HashMap<String, Recovery>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static CHANGED: parking_lot::Condvar = parking_lot::Condvar::new();

pub fn completed(tab: &str) {
    REATTACHING.lock().remove(tab);
    CHANGED.notify_all();
}

pub fn recovering(tab: &str, nonce: &str) -> bool {
    REATTACHING
        .lock()
        .get(tab)
        .is_some_and(|attempt| attempt.nonce == nonce)
}

fn should_reattach(current: bool, live_server: bool, already_recovering: bool) -> bool {
    current && live_server && !already_recovering
}

pub fn try_start(svc: &ServiceContext, tab: &str, nonce: &str, reason: &str) -> bool {
    let Some(sidecar) = super::lifecycle::for_tab(tab) else {
        return false;
    };
    let current = svc
        .mux
        .read()
        .get_tab(tab)
        .is_some_and(|tab| tab.session_binding_nonce.as_deref() == Some(nonce));
    let live = !*sidecar.process.death.borrow();
    if !current {
        return false;
    }
    if super::delivery::attached_for_launch(tab, nonce) {
        super::delivery::refresh(svc, tab);
        attachment_observed(tab, nonce);
        return true;
    }
    let id = uuid::Uuid::new_v4().to_string();
    {
        let mut attempts = REATTACHING.lock();
        if attempts
            .get(tab)
            .is_some_and(|attempt| attempt.nonce != nonce)
        {
            attempts.remove(tab);
        }
        if !should_reattach(current, live, attempts.contains_key(tab)) {
            return true;
        }
        attempts.insert(
            tab.into(),
            Recovery {
                nonce: nonce.into(),
                id: id.clone(),
                result: None,
            },
        );
    }
    log::warn!("[codex-rpc-attach] tab={tab} nonce={nonce} action=reattach reason={reason}");
    svc.log_system_notice(
        "Remote agent detached".into(),
        Some(format!(
            "Tab {tab}: {reason}. Its thread is retained; reconnecting the terminal."
        )),
        Some("error"),
    );
    let (svc, tab, nonce) = (svc.clone(), tab.to_owned(), nonce.to_owned());
    std::thread::spawn(move || {
        let attached = recovered(
            reattach(&svc, &tab, &nonce, &id),
            super::delivery::attached_for_launch(&tab, &nonce),
        );
        if attached {
            super::delivery::refresh(&svc, &tab);
            attachment_observed(&tab, &nonce);
        } else {
            let mut attempts = REATTACHING.lock();
            if let Some(attempt) = attempts
                .get_mut(&tab)
                .filter(|attempt| attempt.nonce == nonce && attempt.id == id)
            {
                attempt.result = Some(false);
                log::error!(
                    "[codex-rpc-attach] tab={tab} nonce={nonce} reattach failed; sends remain held"
                );
            }
        }
        CHANGED.notify_all();
    });
    true
}

fn clear_recovered(attempts: &mut HashMap<String, Recovery>, tab: &str, nonce: &str) {
    if attempts
        .get(tab)
        .is_some_and(|attempt| attempt.nonce == nonce)
    {
        attempts.remove(tab);
    }
}

pub fn attachment_observed(tab: &str, nonce: &str) {
    clear_recovered(&mut REATTACHING.lock(), tab, nonce);
    CHANGED.notify_all();
}

fn recovered(reattach_result: bool, live_attachment: bool) -> bool {
    reattach_result || live_attachment
}

pub fn wait_recovery(tab: &str, nonce: &str) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut attempts = REATTACHING.lock();
    loop {
        if super::delivery::attached_for_launch(tab, nonce) {
            return true;
        }
        match attempts.get(tab) {
            Some(attempt) if attempt.nonce == nonce => {
                if let Some(result) = attempt.result {
                    return result;
                }
            }
            _ => return false,
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return false;
        }
        CHANGED.wait_for(&mut attempts, deadline - now);
    }
}

fn owns_attempt(tab: &str, nonce: &str, id: &str) -> bool {
    REATTACHING
        .lock()
        .get(tab)
        .is_some_and(|attempt| attempt.nonce == nonce && attempt.id == id)
}

fn may_rewrite_remote(proven_provider_exit: bool) -> bool {
    proven_provider_exit
}

fn reattach(svc: &ServiceContext, tab: &str, nonce: &str, id: &str) -> bool {
    if super::delivery::attached_for_launch(tab, nonce) {
        return true;
    }
    let Some(sidecar) = super::lifecycle::for_tab(tab) else {
        return false;
    };
    let Some(pty) = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.pty_id.clone())
    else {
        return false;
    };
    let exited = svc
        .observe_provider_process_tree(tab, &pty)
        .is_some_and(|tree| tree.proves_exit_from_lifecycle_clear());
    if !may_rewrite_remote(exited) {
        log::info!("[codex-rpc-attach] tab={tab} nonce={nonce} recovery=wait_for_protocol_reconnect provider_exit_proven=false");
        super::delivery::retry_attachment(tab, nonce);
        return super::lifecycle::wait_attached(tab, nonce, std::time::Duration::from_secs(30));
    }
    if !crate::service::pty_io::await_bare_shell_prompt(
        &svc.screen_state,
        &pty,
        std::time::Duration::from_secs(30),
    ) {
        return super::delivery::attached_for_launch(tab, nonce);
    }
    let current = svc.mux.read().get_tab(tab).is_some_and(|tab| {
        tab.session_binding_nonce.as_deref() == Some(nonce) && tab.pty_id.as_deref() == Some(&pty)
    });
    if !current
        || *sidecar.process.death.borrow()
        || !super::lifecycle::for_tab(tab)
            .is_some_and(|endpoint| std::sync::Arc::ptr_eq(&sidecar, &endpoint))
        || !owns_attempt(tab, nonce, id)
    {
        return false;
    }
    if super::delivery::retry_attachment(tab, nonce) {
        return true;
    }
    if svc
        .send_launch_command_to_pty_after_ready(&pty, &super::launch::remote_command(&sidecar))
        .is_err()
    {
        super::delivery::attach_failed(tab, nonce);
        return false;
    }
    let attached = super::lifecycle::wait_attached(tab, nonce, std::time::Duration::from_secs(30));
    if attached {
        svc.emit_state_changed();
    }
    attached
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_rpc_attach_failure_retains_server_and_reattaches_current_launch_once() {
        assert!(should_reattach(true, true, false));
        assert!(!should_reattach(false, true, false));
        assert!(!should_reattach(true, false, false));
        assert!(!should_reattach(true, true, true));
    }

    #[test]
    fn codex_rpc_attach_live_connection_wins_over_failed_recovery() {
        assert!(recovered(false, true));
        assert!(recovered(true, true));
        assert!(recovered(true, false));
        assert!(!recovered(false, false));
    }

    #[test]
    fn codex_rpc_attach_current_reconnect_removes_failed_recovery_and_stale_nonce_does_not() {
        let mut attempts = HashMap::from([(
            "tab".into(),
            Recovery {
                nonce: "own".into(),
                id: "attempt".into(),
                result: Some(false),
            },
        )]);
        clear_recovered(&mut attempts, "tab", "stale");
        assert_eq!(attempts.len(), 1);
        clear_recovered(&mut attempts, "tab", "own");
        assert!(attempts.is_empty());
    }

    #[test]
    fn codex_rpc_attach_live_or_unknown_provider_waits_for_protocol_without_shell_write() {
        assert!(!may_rewrite_remote(false));
        assert!(may_rewrite_remote(true));
    }
}
