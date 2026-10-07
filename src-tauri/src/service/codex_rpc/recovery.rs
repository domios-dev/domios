use crate::service::ServiceContext;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

static REATTACHING: LazyLock<Mutex<HashMap<String, (String, Option<bool>)>>> =
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
        .is_some_and(|(current, _)| current == nonce)
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
    super::delivery::disconnected(tab);
    {
        let mut attempts = REATTACHING.lock();
        if !should_reattach(current, live, attempts.contains_key(tab)) {
            return true;
        }
        attempts.insert(tab.into(), (nonce.into(), None));
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
        let attached = reattach(&svc, &tab, &nonce);
        if let Some((current, result)) = REATTACHING.lock().get_mut(&tab) {
            if current == &nonce {
                *result = Some(attached);
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

pub fn wait_recovery(tab: &str, nonce: &str) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut attempts = REATTACHING.lock();
    loop {
        match attempts.get(tab) {
            Some((current, Some(result))) if current == nonce => return *result,
            Some((current, None)) if current == nonce => {}
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
        return false;
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
    super::delivery::retry_attachment(tab, nonce);
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
}
