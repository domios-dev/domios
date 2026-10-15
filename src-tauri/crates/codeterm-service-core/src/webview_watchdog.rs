//! macOS self-heal for a reaped WKWebView renderer (GH #188). Under memory
//! pressure jetsam kills the WebContent (renderer) process while GPU/Networking
//! helpers survive, leaving the native window blank with no auto-reload. wry 0.54
//! has a termination handler but Tauri 2.10 doesn't expose it, so we poll the
//! renderer PID and call `reload()` once it dies. No-op off macOS.

#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
const POLL_INTERVAL: Duration = Duration::from_millis(1000);
#[cfg(target_os = "macos")]
const BACKOFF_MAX: Duration = Duration::from_secs(30);
#[cfg(target_os = "macos")]
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Start the renderer watchdog for the main window. No-op on non-macOS.
pub fn spawn(window: crate::app_runtime::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::thread::Builder::new()
            .name("webview-watchdog".into())
            .spawn(move || run(window));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

#[cfg(target_os = "macos")]
fn run(window: crate::app_runtime::WebviewWindow) {
    let mut ever_alive = false;
    let mut backoff = POLL_INTERVAL;
    loop {
        std::thread::sleep(backoff);
        match probe(&window) {
            Probe::Alive => {
                ever_alive = true;
                backoff = POLL_INTERVAL;
            }
            Probe::Dead if ever_alive => {
                log::warn!("[webview-watchdog] WebContent renderer gone — reloading window");
                let _ = window.reload();
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
            // Renderer not up yet (initial load) — keep polling at base cadence.
            Probe::Dead => backoff = POLL_INTERVAL,
            // Probe couldn't reach the main thread this tick — retry next tick.
            Probe::Unknown => backoff = POLL_INTERVAL,
            // SPI missing or window destroyed — nothing more we can do.
            Probe::Stop => return,
        }
    }
}

#[cfg(target_os = "macos")]
enum Probe {
    Alive,
    Dead,
    Unknown,
    Stop,
}

#[cfg(target_os = "macos")]
fn probe(window: &crate::app_runtime::WebviewWindow) -> Probe {
    let (tx, rx) = std::sync::mpsc::channel::<i32>();
    let posted = window.with_webview(move |wv| {
        let pid = unsafe { web_process_identifier(wv.inner()) };
        let _ = tx.send(pid);
    });
    if posted.is_err() {
        return Probe::Stop;
    }
    match rx.recv_timeout(PROBE_TIMEOUT) {
        Ok(pid) if pid > 0 => Probe::Alive,
        Ok(0) => Probe::Dead,
        // Negative sentinel = SPI unavailable on this WebKit build.
        Ok(_) => Probe::Stop,
        Err(_) => Probe::Unknown,
    }
}

/// Reads `-[WKWebView _webProcessIdentifier]` (renderer PID, 0 when reaped).
/// Returns -1 when the SPI is absent so the caller can disable itself.
#[cfg(target_os = "macos")]
unsafe fn web_process_identifier(wkwebview: *mut std::ffi::c_void) -> i32 {
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, sel};
    if wkwebview.is_null() {
        return -1;
    }
    let obj = wkwebview as *mut AnyObject;
    let selector = sel!(_webProcessIdentifier);
    let responds: bool = msg_send![obj, respondsToSelector: selector];
    if !responds {
        return -1;
    }
    msg_send![obj, _webProcessIdentifier]
}
