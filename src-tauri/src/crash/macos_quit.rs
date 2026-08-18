//! macOS quits (Cmd+Q, app menu, Dock, logout) arrive as AppKit `terminate:` →
//! `applicationWillTerminate:` → tao `LoopDestroyed` → `RunEvent::Exit`, so
//! `RunEvent::ExitRequested` never fires. Hooking `applicationShouldTerminate:`
//! on the live delegate observes the quit before AppKit tears the app down.

use std::ffi::c_char;

use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{class, ffi, msg_send, sel};

/// `NSTerminateNow` — proceed with termination.
const TERMINATE_NOW: usize = 1;
/// NSUInteger return, self, _cmd, sender.
const SHOULD_TERMINATE_TYPES: &[u8] = b"L@:@\0";

type ShouldTerminateImp = extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize;

extern "C-unwind" fn should_terminate(_: *mut AnyObject, _: Sel, _: *mut AnyObject) -> usize {
    observe_quit_request()
}

fn observe_quit_request() -> usize {
    crate::crash::shutdown::mark_shutting_down();
    log::info!("[shutdown] macOS quit observed; clean exit declared before termination");
    TERMINATE_NOW
}

/// Call once from Tauri `setup`, on the main thread, after the delegate exists.
pub fn install_quit_hook() {
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
