use crate::service::ServiceContext;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

static REATTACHING: LazyLock<Mutex<HashMap<String, (String, Option<bool>, bool)>>> =
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
        .is_some_and(|(current, result, _)| current == nonce && *result != Some(true))
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
    {
        let mut attempts = REATTACHING.lock();
        if let Some((current, result, running)) = attempts.get_mut(tab) {
            if current == nonce && *running {
                *result = None;
                return true;
            }
        }
        if attempts
            .get(tab)
            .is_some_and(|(_, result, _)| *result == Some(true))
        {
            attempts.remove(tab);
        }
        if !should_reattach(current, live, attempts.contains_key(tab)) {
            return true;
        }
        attempts.insert(tab.into(), (nonce.into(), None, true));
    }
    log::warn!("[codex-rpc-attach] tab={tab} nonce={nonce} action=reattach reason={reason}");
    svc.create_notification_ext(
        "Remote agent detached".into(),
        Some(format!(
            "Tab {tab}: {reason}. Its thread is retained; reconnecting the terminal."
        )),
        Some("error"),
        None,
        Some(true),
        crate::service::notification::NotificationOrigin::local_tab(tab.into()),
        crate::service::notification::PushIntent {
            level: Some(crate::notification::push::NotifyLevel::Failure),
            event_key: Some(format!("codex-tui-detached:{tab}:{nonce}")),
        },
        None,
    );
    let (svc, tab, nonce) = (svc.clone(), tab.to_owned(), nonce.to_owned());
    std::thread::spawn(move || {
        let attached = recovered(
            reattach(&svc, &tab, &nonce),
            super::delivery::attached_for_launch(&tab, &nonce),
        );
        if attached {
            super::delivery::refresh(&svc, &tab);
        }
        if let Some((current, result, running)) = REATTACHING.lock().get_mut(&tab) {
            if current == &nonce {
                *result = Some(attached);
                *running = false;
            }
        }
        CHANGED.notify_all();
        if !attached {
            log::error!(
                "[codex-rpc-attach] tab={tab} nonce={nonce} reattach failed; sends remain held"
            );
        }
    });
    true
}

pub fn attachment_observed(tab: &str, nonce: &str) {
    if let Some((current, result, _)) = REATTACHING.lock().get_mut(tab) {
        if current == nonce {
            *result = Some(true);
        }
    }
    CHANGED.notify_all();
}

fn recovered(reattach_result: bool, live_attachment: bool) -> bool {
    reattach_result || live_attachment
}

pub fn wait_recovery(tab: &str, nonce: &str) -> bool {
    if super::delivery::attached_for_launch(tab, nonce) {
        return true;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut attempts = REATTACHING.lock();
    loop {
        match attempts.get(tab) {
            Some((current, Some(result), _)) if current == nonce => return *result,
            Some((current, None, _)) if current == nonce => {}
            _ => return false,
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return false;
        }
        CHANGED.wait_for(&mut attempts, deadline - now);
    }
}

fn reattach(svc: &ServiceContext, tab: &str, nonce: &str) -> bool {
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
}
