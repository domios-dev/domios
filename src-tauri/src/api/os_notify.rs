//! OS-level toast notification helpers.
//! Used by other handlers to surface notifications to the user via tauri-plugin-notification.
/// Send an OS-level toast notification if the CodeTerm window is not focused.
pub fn _send_os_notification_if_unfocused(app: &tauri::AppHandle, title: &str, body: Option<&str>) {
    use tauri::Manager;
    let is_focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(true);

    if !is_focused {
        _send_os_notification(app, title, body);
    }
}

/// `NotificationBuilder::show` spawns the real send onto the async runtime and drops
/// its result, so its `Ok` is not evidence of delivery and can never select a fallback.
/// Windows silently discards a toast from a binary running out of `target/`, because the
/// plugin declines to set an AppUserModelID there.
fn native_toast_can_deliver() -> bool {
    #[cfg(windows)]
    {
        match tauri::utils::platform::current_exe() {
            Ok(exe) => windows_toast_reaches_the_shell(exe.parent()),
            Err(_) => false,
        }
    }
    #[cfg(not(windows))]
    {
        true
    }
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

/// Single notification popup via system notification API.
/// Uses tauri-plugin-notification on all platforms — clicking focuses the app.
#[cfg_attr(test, allow(unreachable_code))]
pub fn _send_os_notification(app: &tauri::AppHandle, title: &str, body: Option<&str>) {
    #[cfg(test)]
    {
        let _ = (app, title, body);
        return;
    }

    if native_toast_can_deliver() {
        use tauri_plugin_notification::NotificationExt;
        let mut notif = app.notification().builder().title(title);
        if let Some(b) = body {
            notif = notif.body(b);
        }
        let _ = notif.show();
        return;
    }

    // Fallback: custom WebviewWindow popup where the native toast cannot arrive.
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        use tauri::Manager;

        static NOTIF_ACTIVE: AtomicBool = AtomicBool::new(false);
        static LAST_MSG: parking_lot::Mutex<String> = parking_lot::Mutex::new(String::new());

        let msg = format!("{}: {}", title, body.unwrap_or(""));

        {
            let last = LAST_MSG.lock();
            if *last == msg && NOTIF_ACTIVE.load(Ordering::Relaxed) {
                return;
            }
        }
        *LAST_MSG.lock() = msg.clone();

        let title = title.to_string();
        let body = body.unwrap_or("").to_string();
        let app = app.clone();

        if NOTIF_ACTIVE.load(Ordering::Relaxed) {
            if let Some(win) = app.get_webview_window("codeterm-notif") {
                let html = _build_notif_html(&title, &body);
                let _ = win.eval(&format!(
                    "document.open();document.write({});document.close();",
                    serde_json::to_string(&html).unwrap_or_default()
                ));
                return;
            }
        }

        NOTIF_ACTIVE.store(true, Ordering::Relaxed);

        std::thread::spawn(move || {
            let (wa_x, wa_y, wa_w, wa_h) = _get_workarea();
            let win_w = 320.0;
            let win_h = _calc_notif_height(&body, win_w);
            let margin = 16.0;

            let html = _build_notif_html(&title, &body);

            if let Ok(win) = tauri::WebviewWindowBuilder::new(
                &app,
                "codeterm-notif",
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
                let _ = win.eval(&format!(
                    "document.open();document.write({});document.close();",
                    serde_json::to_string(&html).unwrap_or_default()
                ));

                let app_clone = app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(4));
                    if let Some(w) = app_clone.get_webview_window("codeterm-notif") {
                        let _ = w.close();
                    }
                    NOTIF_ACTIVE.store(false, Ordering::Relaxed);
                });
            } else {
                NOTIF_ACTIVE.store(false, Ordering::Relaxed);
            }
        });
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
        </body></html>"#,
        title.replace('<', "&lt;").replace('>', "&gt;"),
        body.replace('<', "&lt;").replace('>', "&gt;"),
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
