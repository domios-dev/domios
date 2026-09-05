//! OS-level toast notification helpers.
//! Used by other handlers to surface notifications to the user via tauri-plugin-notification.
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use tauri_plugin_notification::{NotificationExt, PermissionState};

const PERMISSION_PROMPT: u8 = 0;
const PERMISSION_GRANTED: u8 = 1;
const PERMISSION_DENIED: u8 = 2;

/// The answer the OS has given this process, for the whole process. `Prompt`
/// until something asks, which is why enabling asks rather than the first send.
static ESTABLISHED_PERMISSION: AtomicU8 = AtomicU8::new(PERMISSION_PROMPT);

/// Popup dedupe and liveness; cleared from the window's own Destroyed event.
static NOTIF_ACTIVE: AtomicBool = AtomicBool::new(false);
static LAST_MSG: parking_lot::Mutex<String> = parking_lot::Mutex::new(String::new());

fn permission_code(state: PermissionState) -> u8 {
    match state {
        PermissionState::Granted => PERMISSION_GRANTED,
        PermissionState::Denied => PERMISSION_DENIED,
        _ => PERMISSION_PROMPT,
    }
}

fn permission_from_code(code: u8) -> PermissionState {
    match code {
        PERMISSION_GRANTED => PermissionState::Granted,
        PERMISSION_DENIED => PermissionState::Denied,
        _ => PermissionState::Prompt,
    }
}

/// What the OS has already answered. Nothing asked yet reads `Prompt`, which is
/// honest: it is not a refusal, and it is not a grant either.
pub fn established_toast_permission() -> PermissionState {
    permission_from_code(ESTABLISHED_PERMISSION.load(Ordering::Relaxed))
}

fn record_toast_permission(state: PermissionState) {
    ESTABLISHED_PERMISSION.store(permission_code(state), Ordering::Relaxed);
}

/// The one seam onto the OS. tauri-plugin-notification's desktop build answers
/// `Granted` from a stub on every platform, so Windows reads the shell's own
/// toast switch instead of trusting the stub; a truthful macOS probe still
/// needs to replace this function's other platform arm.
#[cfg(windows)]
fn ask_the_os(_app: &tauri::AppHandle) -> PermissionState {
    windows_shell_permission()
}

#[cfg(not(windows))]
fn ask_the_os(app: &tauri::AppHandle) -> PermissionState {
    app.notification()
        .request_permission()
        .unwrap_or(PermissionState::Denied)
}

/// The same seam, reading rather than asking: it must never raise a prompt.
#[cfg(windows)]
fn read_the_os(_app: &tauri::AppHandle) -> Option<PermissionState> {
    Some(windows_shell_permission())
}

#[cfg(not(windows))]
fn read_the_os(app: &tauri::AppHandle) -> Option<PermissionState> {
    app.notification().permission_state().ok()
}

/// The real Windows answer: the shell's own Notifications switch, never the
/// plugin's hardcoded `Granted` stub. An absent registry entry is the factory
/// state, which presents toasts.
#[cfg(windows)]
fn windows_shell_permission() -> PermissionState {
    permission_from_toast_switch(read_user_toast_switch())
}

/// Pure translation of the raw switch value, split out so the branch can be
/// exercised without touching the registry or the plugin.
#[cfg(any(windows, test))]
fn permission_from_toast_switch(user_toast_switch: Option<u32>) -> PermissionState {
    if shell_presents_toasts(user_toast_switch) {
        PermissionState::Granted
    } else {
        PermissionState::Denied
    }
}

/// Permission belongs to the enabling gesture: switching a surface ON asks the
/// OS, switching it OFF asks nothing, because withdrawing needs no consent.
fn establish_with(
    enabling: bool,
    ask: impl FnOnce() -> PermissionState,
) -> Option<PermissionState> {
    if !enabling {
        return None;
    }
    Some(ask())
}

/// The enabling gesture, wired to the OS: on asks and remembers the answer, off
/// reports what is already known without raising a prompt.
pub fn establish_toast_permission(app: &tauri::AppHandle, enabling: bool) -> PermissionState {
    match establish_with(enabling, || ask_the_os(app)) {
        Some(state) => {
            record_toast_permission(state);
            state
        }
        None => established_toast_permission(),
    }
}

/// Fill an unasked slot from the OS without prompting. An answer already given
/// this process is never overwritten, so a refusal outlives every later send.
fn refresh_unasked_permission(app: &tauri::AppHandle) {
    if established_toast_permission() != PermissionState::Prompt {
        return;
    }
    if let Some(state) = read_the_os(app) {
        record_toast_permission(state);
    }
}

/// What the OS says right now, without asking the user for anything.
pub fn toast_permission(app: &tauri::AppHandle) -> PermissionState {
    refresh_unasked_permission(app);
    established_toast_permission()
}

/// Deliverability is permission AND a platform channel that carries the toast.
/// Neither half alone is evidence, so neither half alone may answer yes.
fn toast_can_deliver(permission: PermissionState, channel_open: bool) -> bool {
    channel_open && permission == PermissionState::Granted
}

/// The platform half only: Windows discards a toast that carries no
/// AppUserModelID, and every toast at all while the user's master switch is off.
fn platform_toast_channel_open() -> bool {
    #[cfg(windows)]
    {
        let addressed = match tauri::utils::platform::current_exe() {
            Ok(exe) => windows_toast_reaches_the_shell(exe.parent()),
            Err(_) => false,
        };
        addressed && shell_presents_toasts(read_user_toast_switch())
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// The shell's own switch, read as a value instead of inferred from a send that
/// cannot fail. An absent entry is the factory state; only an explicit 0 discards.
#[cfg(any(windows, test))]
fn shell_presents_toasts(user_toast_switch: Option<u32>) -> bool {
    user_toast_switch != Some(0)
}

/// Where Windows records the master Notifications switch. With it off the shell
/// accepts every toast and presents none, and no send Result ever says so.
#[cfg(windows)]
fn read_user_toast_switch() -> Option<u32> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    fn wide(text: &str) -> Vec<u16> {
        OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let subkey = wide(r"SOFTWARE\Microsoft\Windows\CurrentVersion\PushNotifications");
    let value = wide("ToastEnabled");
    let mut switch: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            std::ptr::addr_of_mut!(switch).cast(),
            &mut size,
        )
    };
    (status == ERROR_SUCCESS).then_some(switch)
}

fn native_toast_can_deliver() -> bool {
    toast_can_deliver(
        established_toast_permission(),
        platform_toast_channel_open(),
    )
}

/// Mirrors the plugin's own AppUserModelID condition: it sets the id only outside
/// `target/debug` and `target/release`, and Windows drops a toast that carries none.
#[cfg(any(windows, test))]
fn windows_toast_reaches_the_shell(exe_dir: Option<&std::path::Path>) -> bool {
    let sep = std::path::MAIN_SEPARATOR;
    match exe_dir.map(|dir| dir.display().to_string()) {
        Some(dir) => {
            !(dir.ends_with(&format!("{sep}target{sep}debug"))
                || dir.ends_with(&format!("{sep}target{sep}release")))
        }
        None => false,
    }
}

/// Where a toast ended up, once the gate and the send itself have both answered.
#[derive(Debug, PartialEq, Eq)]
enum ToastRoute {
    Native,
    FallbackWindow,
}

/// The whole send decision. A desktop send answers `Ok` on hand-off and never on
/// arrival, so delivery rests on the gate; a real failure still routes to fallback.
fn route_toast<E>(can_deliver: bool, send: impl FnOnce() -> Result<(), E>) -> ToastRoute {
    if can_deliver && send().is_ok() {
        return ToastRoute::Native;
    }
    ToastRoute::FallbackWindow
}

#[cfg(test)]
mod delivery_tests {
    use super::windows_toast_reaches_the_shell;
    use std::path::{Path, PathBuf};

    fn native(parts: &[&str]) -> PathBuf {
        parts.iter().collect()
    }

    #[test]
    fn an_installed_binary_may_use_the_native_toast() {
        assert!(windows_toast_reaches_the_shell(Some(&native(&[
            "C:",
            "Program Files",
            "CodeTerm"
        ]))));
    }

    #[test]
    fn a_binary_running_out_of_target_falls_back_instead_of_vanishing() {
        for profile in ["debug", "release"] {
            let dir = native(&["D:", "Developer", "codeterm", "target", profile]);
            assert!(
                !windows_toast_reaches_the_shell(Some(&dir)),
                "the plugin sets no AppUserModelID under target/{profile}, so the toast is dropped"
            );
        }
    }

    #[test]
    fn an_unresolvable_exe_directory_falls_back() {
        assert!(!windows_toast_reaches_the_shell(None::<&Path>));
    }
}

#[cfg(test)]
mod notify_permission_tests;

#[cfg(test)]
mod toast_delivery_tests;

/// Single notification popup via system notification API.
/// Uses tauri-plugin-notification on all platforms — clicking focuses the app.
#[cfg_attr(test, allow(unreachable_code))]
pub fn _send_os_notification(app: &tauri::AppHandle, title: &str, body: Option<&str>) {
    #[cfg(test)]
    {
        let _ = (app, title, body);
        return;
    }

    refresh_unasked_permission(app);
    let route = route_toast(native_toast_can_deliver(), || {
        let mut notif = app.notification().builder().title(title);
        if let Some(b) = body {
            notif = notif.body(b);
        }
        notif.show()
    });
    if route == ToastRoute::Native {
        return;
    }

    // Fallback: custom WebviewWindow popup where the native toast cannot arrive.
    {
        let msg = format!("{}: {}", title, body.unwrap_or(""));

        {
            let last = LAST_MSG.lock();
            if *last == msg && NOTIF_ACTIVE.load(Ordering::Relaxed) {
                return;
            }
        }
        *LAST_MSG.lock() = msg.clone();

        // `swap` so the decision to reuse or build is taken once, and the popup
        // is marked live before the main thread is asked to raise it.
        let request = NotifRequest {
            title: title.to_string(),
            body: body.unwrap_or("").to_string(),
            reuse_existing: NOTIF_ACTIVE.swap(true, Ordering::Relaxed),
        };
        raise_popup(app, request);
    }
}

/// What the main thread is handed to raise the popup.
///
/// Data only, and `Serialize` is what enforces that: a `WebviewWindow` field would
/// fail to derive. A `Send` bound would not catch it — `tauri-runtime-wry` declares
/// `unsafe impl Send for DispatcherMainThreadContext`, so a webview handle is
/// `Send` while transitively holding tao's `!Send` `Rc<EventLoopRunner>`. Cloning
/// or dropping one off the main thread is what aborted the daemon on 2026-09-05.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct NotifRequest {
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) reuse_existing: bool,
}

pub(crate) const NOTIF_WINDOW_LABEL: &str = "codeterm-notif";

/// How long the popup stays up. The window dismisses itself from its own document
/// timer, so no thread waits on it and no handle outlives the closure below.
pub(crate) const NOTIF_DWELL_MS: u32 = 4000;

fn document_write(html: &str) -> String {
    format!(
        "document.open();document.write({});document.close();",
        serde_json::to_string(html).unwrap_or_default()
    )
}

/// Every webview handle in this path is obtained, used and dropped inside this
/// closure, on the main thread. The caller runs on a tokio worker, a socket
/// thread or the updater runtime and never touches one.
fn raise_popup(app: &tauri::AppHandle, request: NotifRequest) {
    let poster = app.clone();
    let app = app.clone();
    let posted = poster.run_on_main_thread(move || {
        use tauri::Manager;

        let html = _build_notif_html(&request.title, &request.body);

        if request.reuse_existing {
            if let Some(win) = app.get_webview_window(NOTIF_WINDOW_LABEL) {
                let _ = win.eval(&document_write(&html));
                return;
            }
        }

        let (wa_x, wa_y, wa_w, wa_h) = _get_workarea();
        let win_w = 320.0;
        let win_h = _calc_notif_height(&request.body, win_w);
        let margin = 16.0;

        match tauri::WebviewWindowBuilder::new(
            &app,
            NOTIF_WINDOW_LABEL,
            tauri::WebviewUrl::App("about:blank".into()),
        )
        .title("")
        .inner_size(win_w, win_h)
        .position(wa_x + wa_w - win_w - margin, wa_y + wa_h - win_h - margin)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .build()
        {
            Ok(win) => {
                // Cleared when the window is actually gone, not when a timer says so.
                win.on_window_event(|event| {
                    if matches!(event, tauri::WindowEvent::Destroyed) {
                        NOTIF_ACTIVE.store(false, Ordering::Relaxed);
                    }
                });
                let _ = win.eval(&document_write(&html));
            }
            Err(_) => NOTIF_ACTIVE.store(false, Ordering::Relaxed),
        }
    });
    // A dead event loop would otherwise leave the popup marked live forever, and
    // the dedupe above would suppress every later notification.
    if posted.is_err() {
        NOTIF_ACTIVE.store(false, Ordering::Relaxed);
    }
}

fn _build_notif_html(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html><html><head><style>
        *{{margin:0;padding:0;box-sizing:border-box}}
        body{{background:#1c2128;color:#c9d1d9;font-family:'Segoe UI',sans-serif;
              padding:12px 16px;border:1px solid #30363d;border-radius:8px;
              overflow:hidden;cursor:pointer}}
        body:hover{{background:#21262d}}
        .t{{font-size:13px;font-weight:600;color:#f0f6fc;margin-bottom:4px}}
        .b{{font-size:12px;color:#8b949e}}
        .i{{float:left;margin-right:10px;font-size:18px;color:#f0883e}}
        </style></head><body>
        <span class="i">&#10035;</span>
        <div class="t">{}</div>
        <div class="b">{}</div>
        <script>setTimeout(function(){{window.close()}},{})</script>
        </body></html>"#,
        title.replace('<', "&lt;").replace('>', "&gt;"),
        body.replace('<', "&lt;").replace('>', "&gt;"),
        NOTIF_DWELL_MS,
    )
}

/// Estimate notification popup height based on body text length.
fn _calc_notif_height(body: &str, win_w: f64) -> f64 {
    let chars_per_line = ((win_w - 60.0) / 7.0).max(1.0) as usize;
    let lines = if body.is_empty() {
        0
    } else {
        (body.len() / chars_per_line).max(1)
    };
    let base = 50.0;
    let line_h = 16.0;
    (base + lines as f64 * line_h).max(60.0).min(200.0)
}

/// Returns workarea (x, y, width, height) — desktop minus taskbar.
#[allow(dead_code)]
fn _get_workarea() -> (f64, f64, f64, f64) {
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct RECT {
            left: i32,
            top: i32,
            right: i32,
            bottom: i32,
        }
        #[link(name = "user32")]
        extern "system" {
            fn SystemParametersInfoW(
                action: u32,
                param: u32,
                pvparam: *mut RECT,
                winini: u32,
            ) -> i32;
        }
        unsafe {
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            };
            SystemParametersInfoW(0x0030, 0, &mut rect, 0);
            let dpi = {
                #[link(name = "user32")]
                extern "system" {
                    fn GetDpiForSystem() -> u32;
                }
                GetDpiForSystem()
            };
            let scale = dpi as f64 / 96.0;
            let x = rect.left as f64 / scale;
            let y = rect.top as f64 / scale;
            let w = (rect.right - rect.left) as f64 / scale;
            let h = (rect.bottom - rect.top) as f64 / scale;
            (x, y, w, h)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        (0.0, 0.0, 1920.0, 1080.0)
    }
}

#[cfg(test)]
mod tests;
