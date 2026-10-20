use codeterm_types::native_host::{NativeHostRejection, NativeHostUncertainty};
use std::{
    sync::atomic::{AtomicU8, AtomicUsize, Ordering},
    time::Duration,
};
use tauri::Manager;

type AppHandle = codeterm_client_shell::app_runtime::AppHandle;

const MAX_QUEUED_MAIN_THREAD_CALLS: usize = 64;
const MAIN_THREAD_ACK_DEADLINE: Duration = Duration::from_secs(10);
const MAIN_THREAD_QUEUED: u8 = 0;
const MAIN_THREAD_RUNNING: u8 = 1;
const MAIN_THREAD_CANCELLED: u8 = 2;
const MAIN_THREAD_COMPLETED: u8 = 3;
static QUEUED_MAIN_THREAD_CALLS: AtomicUsize = AtomicUsize::new(0);
struct MainThreadQueuePermit;

impl MainThreadQueuePermit {
    fn acquire() -> Option<Self> {
        QUEUED_MAIN_THREAD_CALLS
            .try_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                (pending < MAX_QUEUED_MAIN_THREAD_CALLS).then_some(pending + 1)
            })
            .ok()
            .map(|_| Self)
    }
}

impl Drop for MainThreadQueuePermit {
    fn drop(&mut self) {
        QUEUED_MAIN_THREAD_CALLS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct TauriShell(pub AppHandle);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeShellFailure {
    Refused(NativeHostRejection),
    Uncertain(NativeHostUncertainty),
}

enum MainThreadCallError {
    QueueFull,
    Scheduling,
    Action,
    DeadlineBeforeDispatch,
    UncertainAfterDispatch,
}

/// The native host protocol reports only the failure class, never the action's error text.
struct ActionFailed;

fn failed(_: impl std::fmt::Display) -> ActionFailed {
    ActionFailed
}

impl TauriShell {
    fn on_main_call<T: Send + 'static>(
        &self,
        action: impl FnOnce(AppHandle) -> Result<T, ActionFailed> + Send + 'static,
    ) -> Result<T, MainThreadCallError> {
        let app = self.0.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let phase = std::sync::Arc::new(AtomicU8::new(MAIN_THREAD_QUEUED));
        let scheduled_phase = phase.clone();
        let permit = MainThreadQueuePermit::acquire().ok_or(MainThreadCallError::QueueFull)?;
        self.0
            .run_on_main_thread(move || {
                let _permit = permit;
                if scheduled_phase
                    .compare_exchange(
                        MAIN_THREAD_QUEUED,
                        MAIN_THREAD_RUNNING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_err()
                {
                    return;
                }
                let _ = sender.send(action(app));
                scheduled_phase.store(MAIN_THREAD_COMPLETED, Ordering::Release);
            })
            .map_err(|_| MainThreadCallError::Scheduling)?;
        match receiver.recv_timeout(MAIN_THREAD_ACK_DEADLINE) {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(ActionFailed)) => Err(MainThreadCallError::Action),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if phase
                    .compare_exchange(
                        MAIN_THREAD_QUEUED,
                        MAIN_THREAD_CANCELLED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    Err(MainThreadCallError::DeadlineBeforeDispatch)
                } else {
                    match receiver.try_recv() {
                        Ok(Ok(result)) => Ok(result),
                        Ok(Err(ActionFailed)) => Err(MainThreadCallError::Action),
                        Err(_) => Err(MainThreadCallError::UncertainAfterDispatch),
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                if phase
                    .compare_exchange(
                        MAIN_THREAD_QUEUED,
                        MAIN_THREAD_CANCELLED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    Err(MainThreadCallError::DeadlineBeforeDispatch)
                } else {
                    Err(MainThreadCallError::UncertainAfterDispatch)
                }
            }
        }
    }

    fn on_main_native<T: Send + 'static>(
        &self,
        action: impl FnOnce(AppHandle) -> Result<T, ActionFailed> + Send + 'static,
    ) -> Result<T, NativeShellFailure> {
        self.on_main_call(action).map_err(|error| match error {
            MainThreadCallError::QueueFull => {
                NativeShellFailure::Refused(NativeHostRejection::QueueFull)
            }
            MainThreadCallError::Scheduling => {
                NativeShellFailure::Refused(NativeHostRejection::HostRejected)
            }
            MainThreadCallError::DeadlineBeforeDispatch => {
                NativeShellFailure::Refused(NativeHostRejection::DeadlineBeforeDispatch)
            }
            MainThreadCallError::Action | MainThreadCallError::UncertainAfterDispatch => {
                NativeShellFailure::Uncertain(NativeHostUncertainty::DisconnectedAfterDispatch)
            }
        })
    }

    pub(crate) fn focus_window_native(&self, label: String) -> Result<(), NativeShellFailure> {
        self.on_main_native(move |app| {
            if label == "main" {
                return crate::startup::desktop_lifecycle::show_or_create(&app).map_err(failed);
            }
            if let Some(window) = app.get_webview_window(&label) {
                window.show().map_err(failed)?;
                window.unminimize().map_err(failed)?;
                window.set_focus().map_err(failed)?;
            }
            Ok(())
        })
    }

    pub(crate) fn window_exists_native(&self, label: String) -> Result<bool, NativeShellFailure> {
        self.on_main_native(move |app| Ok(app.get_webview_window(&label).is_some()))
    }

    pub(crate) fn open_webview_native(
        &self,
        label: String,
        url: url::Url,
        title: String,
        width: f64,
        height: f64,
    ) -> Result<(), NativeShellFailure> {
        self.on_main_native(move |app| {
            tauri::WebviewWindowBuilder::new(&app, label, tauri::WebviewUrl::External(url))
                .title(title)
                .inner_size(width, height)
                .build()
                .map_err(failed)?;
            Ok(())
        })
    }
}
