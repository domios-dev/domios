use tauri::{Emitter, WebviewWindow, Wry};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_NCDESTROY, WM_SIZE},
    },
};

const SUBCLASS_ID: usize = 0x4354_525a;
pub const RESIZE_COMMIT_EVENT: &str = "terminal-window-resize-commit";
pub const RESIZE_START_EVENT: &str = "terminal-window-resize-start";

struct ResizeHookState {
    window: WebviewWindow<Wry>,
    in_size_move: bool,
}

unsafe extern "system" fn resize_subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    state_ptr: usize,
) -> LRESULT {
    let state = &mut *(state_ptr as *mut ResizeHookState);
    match message {
        WM_ENTERSIZEMOVE => {
            state.in_size_move = true;
            let _ = state.window.emit(RESIZE_START_EVENT, ());
        }
        WM_EXITSIZEMOVE => {
            state.in_size_move = false;
            let _ = state.window.emit(RESIZE_COMMIT_EVENT, ());
        }
        WM_SIZE if !state.in_size_move => {
            let _ = state.window.emit(RESIZE_COMMIT_EVENT, ());
        }
        WM_NCDESTROY => {
            RemoveWindowSubclass(hwnd, Some(resize_subclass_proc), subclass_id);
            drop(Box::from_raw(state_ptr as *mut ResizeHookState));
        }
        _ => {}
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

pub fn install(window: &WebviewWindow<Wry>) -> Result<(), Box<dyn std::error::Error>> {
    let hwnd = window.hwnd()?.0 as HWND;
    let state = Box::into_raw(Box::new(ResizeHookState {
        window: window.clone(),
        in_size_move: false,
    }));
    let installed = unsafe {
        SetWindowSubclass(
            hwnd,
            Some(resize_subclass_proc),
            SUBCLASS_ID,
            state as usize,
        )
    };
    if installed == 0 {
        unsafe { drop(Box::from_raw(state)) };
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
