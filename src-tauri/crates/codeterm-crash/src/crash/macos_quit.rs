//! macOS quits (Cmd+Q, app menu, Dock, logout) arrive as AppKit `terminate:` →
//! `applicationWillTerminate:` → tao `LoopDestroyed` → `RunEvent::Exit`, so
//! `RunEvent::ExitRequested` never fires. Hooking `applicationShouldTerminate:`
//! on the live delegate observes the quit before AppKit tears the app down.

use std::ffi::c_char;

use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{class, ffi, msg_send, sel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitDecision {
    Cancel,
    Proceed,
}

const TERMINATE_CANCEL: usize = 0;
const TERMINATE_NOW: usize = 1;
/// NSUInteger return, self, _cmd, sender.
const SHOULD_TERMINATE_TYPES: &[u8] = b"L@:@\0";

type ShouldTerminateImp = extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize;

extern "C-unwind" fn should_terminate(_: *mut AnyObject, _: Sel, _: *mut AnyObject) -> usize {
    dispatch_quit_callback(configured_quit_callback())
}

static QUIT_CALLBACK: std::sync::OnceLock<fn() -> QuitDecision> = std::sync::OnceLock::new();

fn configured_quit_callback() -> fn() -> QuitDecision {
    *QUIT_CALLBACK.get_or_init(server_quit_decision)
}

fn dispatch_quit_decision(decision: QuitDecision) -> usize {
    match decision {
        QuitDecision::Cancel => TERMINATE_CANCEL,
        QuitDecision::Proceed => TERMINATE_NOW,
    }
}

fn dispatch_quit_callback(on_quit: fn() -> QuitDecision) -> usize {
    dispatch_quit_decision(on_quit())
}

fn server_quit_decision() -> QuitDecision {
    crate::crash::shutdown::mark_shutting_down();
    log::info!("[shutdown] macOS quit observed; clean exit declared before termination");
    QuitDecision::Proceed
}

/// Installs the server-owned quit marker behavior.
pub fn install_quit_hook() {
    install_quit_hook_with(server_quit_decision);
}

/// Registers a synchronous decision callback; it must not block on async shutdown.
pub fn install_quit_hook_with(on_quit: fn() -> QuitDecision) {
    let _ = QUIT_CALLBACK.set(on_quit);
    // SAFETY: main thread during setup; NSApp and its delegate are live by then.
    unsafe {
        let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if ns_app.is_null() {
            log::warn!("[shutdown] no NSApplication; macOS quit hook not installed");
            return;
        }
        let delegate: *mut AnyObject = msg_send![ns_app, delegate];
        if delegate.is_null() {
            log::warn!("[shutdown] no NSApplication delegate; macOS quit hook not installed");
            return;
        }
        let delegate_obj: &AnyObject = &*delegate;
        let cls: &AnyClass = delegate_obj.class();
        let selector = sel!(applicationShouldTerminate:);
        if ffi::class_respondsToSelector(cls, selector).as_bool() {
            log::warn!(
                "[shutdown] delegate already answers applicationShouldTerminate:; hook skipped"
            );
            return;
        }
        let imp = std::mem::transmute::<ShouldTerminateImp, Imp>(should_terminate);
        let added = ffi::class_addMethod(
            cls as *const AnyClass as *mut AnyClass,
            selector,
            imp,
            SHOULD_TERMINATE_TYPES.as_ptr() as *const c_char,
        );
        if added.as_bool() {
            log::info!(
                "[shutdown] macOS quit hook installed on {}",
                cls.name().to_string_lossy()
            );
        } else {
            log::error!(
                "[shutdown] macOS quit hook install failed; quit will not declare a clean exit"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cancel_quit() -> QuitDecision {
        QuitDecision::Cancel
    }

    fn proceed_quit() -> QuitDecision {
        QuitDecision::Proceed
    }

    #[test]
    fn appkit_dispatch_obeys_the_typed_quit_decision() {
        assert_eq!(dispatch_quit_callback(cancel_quit), TERMINATE_CANCEL);
        assert_eq!(dispatch_quit_callback(proceed_quit), TERMINATE_NOW);
    }
}
