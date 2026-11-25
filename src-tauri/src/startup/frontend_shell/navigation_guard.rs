use super::{
    external_link, navigation_allowed, same_navigation_document, start_failure_url, starting_url,
    ShellSupervisor,
};
use codeterm_client_shell::app_runtime::{AppHandle, WebviewWindow};
use parking_lot::Mutex;
use tauri::Manager;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GuardedShellDocument {
    Starting,
    Reconnecting,
    StartFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NavigationPermit {
    document: GuardedShellDocument,
    operation: u64,
}

#[derive(Default)]
struct GuardState {
    frontend_active: bool,
    permit: Option<NavigationPermit>,
    current_guarded_document: Option<GuardedShellDocument>,
    next_operation: u64,
}

#[derive(Default)]
pub(super) struct ShellNavigationGuard {
    state: Mutex<GuardState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ShellNavigationState {
    pub(super) frontend_active: bool,
    pub(super) permit: Option<GuardedShellDocument>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ShellNavigationDecision {
    pub(super) allowed: bool,
    pub(super) open_externally: bool,
}

impl ShellNavigationGuard {
    pub(super) fn reset(&self) {
        let mut state = self.state.lock();
        state.frontend_active = false;
        state.permit = None;
        state.current_guarded_document = None;
    }

    pub(super) fn mark_frontend_active(&self) {
        let mut state = self.state.lock();
        state.frontend_active = true;
        state.permit = None;
        state.current_guarded_document = None;
    }

    pub(super) fn grant_for_navigation(
        &self,
        document: GuardedShellDocument,
        same_document: bool,
    ) -> Option<u64> {
        let mut state = self.state.lock();
        if same_document
            || state.current_guarded_document == Some(document)
            || !state.frontend_active
        {
            return None;
        }
        state.next_operation = state.next_operation.wrapping_add(1).max(1);
        let operation = state.next_operation;
        state.permit = Some(NavigationPermit {
            document,
            operation,
        });
        Some(operation)
    }

    pub(super) fn revoke_operation(&self, operation: u64) {
        let mut state = self.state.lock();
        if state
            .permit
            .is_some_and(|permit| permit.operation == operation)
        {
            state.permit = None;
        }
    }

    pub(super) fn decide(
        &self,
        url: &Url,
        origin: Option<&str>,
        api_origin: Option<&str>,
        unavailable: &Url,
        fallback: &Url,
    ) -> ShellNavigationDecision {
        let mut state = self.state.lock();
        let guard = guarded_shell_document(url, unavailable);
        let permit = guard.and_then(|document| {
            if state
                .permit
                .is_some_and(|permit| permit.document == document)
            {
                state.permit = None;
                Some(document)
            } else if state.current_guarded_document == Some(document) {
                Some(document)
            } else {
                None
            }
        });
        let decision = shell_navigation_decision(
            url,
            origin,
            api_origin,
            unavailable,
            fallback,
            ShellNavigationState {
                frontend_active: state.frontend_active,
                permit,
            },
        );
        if decision.allowed {
            state.current_guarded_document = guard;
        }
        decision
    }

    #[cfg(test)]
    pub(super) fn pending_permit(&self) -> Option<GuardedShellDocument> {
        self.state.lock().permit.map(|permit| permit.document)
    }

    #[cfg(test)]
    pub(super) fn is_frontend_active(&self) -> bool {
        self.state.lock().frontend_active
    }
}

pub(super) fn same_document_without_fragment(current: &Url, target: &Url) -> bool {
    let mut current = current.clone();
    current.set_fragment(None);
    let mut target = target.clone();
    target.set_fragment(None);
    current == target
}

pub(super) fn guarded_shell_document(url: &Url, unavailable: &Url) -> Option<GuardedShellDocument> {
    if starting_url().is_ok_and(|starting| same_navigation_document(url, &starting)) {
        Some(GuardedShellDocument::Starting)
    } else if same_navigation_document(url, unavailable) {
        Some(GuardedShellDocument::Reconnecting)
    } else if start_failure_url(None).is_ok_and(|failed| same_navigation_document(url, &failed)) {
        Some(GuardedShellDocument::StartFailed)
    } else {
        None
    }
}

pub(super) fn shell_navigation_decision(
    url: &Url,
    origin: Option<&str>,
    api_origin: Option<&str>,
    unavailable: &Url,
    fallback: &Url,
    state: ShellNavigationState,
) -> ShellNavigationDecision {
    if let Some(document) = guarded_shell_document(url, unavailable) {
        let permitted = state.permit == Some(document);
        if state.frontend_active && !permitted {
            // Windows starting document is http; cancelling it must not open a browser.
            return ShellNavigationDecision {
                allowed: false,
                open_externally: false,
            };
        }
        return ShellNavigationDecision {
            allowed: true,
            open_externally: false,
        };
    }
    let allowed = navigation_allowed(url, origin, api_origin, unavailable, fallback);
    ShellNavigationDecision {
        allowed,
        open_externally: !allowed && external_link(url, origin, api_origin).is_some(),
    }
}

pub(super) fn evaluate_main_navigation(
    app: &AppHandle,
    url: &Url,
    origin: Option<&str>,
    api_origin: Option<&str>,
    unavailable: &Url,
    fallback: &Url,
) -> ShellNavigationDecision {
    if let Some(supervisor) = app.try_state::<ShellSupervisor>() {
        supervisor
            .navigation_guard
            .decide(url, origin, api_origin, unavailable, fallback)
    } else {
        shell_navigation_decision(
            url,
            origin,
            api_origin,
            unavailable,
            fallback,
            ShellNavigationState {
                frontend_active: false,
                permit: None,
            },
        )
    }
}

pub(super) fn mark_frontend_active(app: &AppHandle) {
    if let Some(supervisor) = app.try_state::<ShellSupervisor>() {
        supervisor.navigation_guard.mark_frontend_active();
    }
}

pub(super) fn navigate_main_guarded(
    window: &WebviewWindow,
    url: Url,
    document: GuardedShellDocument,
) -> Result<(), String> {
    let main = window.label() == "main";
    let operation = if main {
        let same_document = window
            .url()
            .ok()
            .is_some_and(|current| same_document_without_fragment(&current, &url));
        window
            .app_handle()
            .try_state::<ShellSupervisor>()
            .and_then(|supervisor| {
                supervisor
                    .navigation_guard
                    .grant_for_navigation(document, same_document)
            })
    } else {
        None
    };
    match window.navigate(url) {
        Ok(()) => Ok(()),
        Err(error) => {
            if let Some(operation) = operation {
                if let Some(supervisor) = window.app_handle().try_state::<ShellSupervisor>() {
                    supervisor.navigation_guard.revoke_operation(operation);
                }
            }
            Err(error.to_string())
        }
    }
}
