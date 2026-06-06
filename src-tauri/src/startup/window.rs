//! Window styling + system tray setup.

use tauri::Manager;

/// Set window icon and apply platform-specific styling.
pub(crate) fn setup_window_styling(window: &tauri::WebviewWindow) {
    let icon_bytes = include_bytes!("../../icons/icon.png");
    if let Ok(mut reader) = png::Decoder::new(std::io::Cursor::new(icon_bytes)).read_info() {
        let info = reader.info().clone();
        let mut rgba = vec![0u8; reader.output_buffer_size()];
        if reader.next_frame(&mut rgba).is_ok() {
            let icon = tauri::image::Image::new_owned(rgba, info.width, info.height);
            let _ = window.set_icon(icon);
        }
    }

    // macOS: rounded corners for frameless window
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSWindow, NSWindowStyleMask, NSWindowTitleVisibility};
        let ns_win_raw = match window.ns_window() {
            Ok(h) => h,
            Err(_) => {
                log::warn!("[window] ns_window() failed — skipping macOS styling");
                return;
            }
        };
        let ns_win = ns_win_raw as *mut objc2::runtime::AnyObject;
        unsafe {
            let ns_window: &NSWindow = &*(ns_win as *const NSWindow);
            let mut mask = ns_window.styleMask();
            mask |= NSWindowStyleMask::Titled;
            mask |= NSWindowStyleMask::FullSizeContentView;
            ns_window.setStyleMask(mask);
            ns_window.setTitlebarAppearsTransparent(true);
            ns_window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
            ns_window.setHasShadow(true);
            // Hide traffic light buttons
            use objc2::msg_send;
            for i in 0..3_isize {
                let btn: *mut objc2::runtime::AnyObject =
                    msg_send![ns_window, standardWindowButton: i];
                if !btn.is_null() {
                    let _: () = msg_send![btn, setHidden: true];
                }
            }
        }
    }
}

/// Build and configure the system tray icon and menu.
pub(crate) fn build_system_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItemBuilder::with_id("show", "Show CodeTerm").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&show)
        .separator()
        .item(&quit)
        .build()?;

    let tray_icon = {
        // macOS: template image (white on transparent, auto-adapts to light/dark)
        #[cfg(target_os = "macos")]
        let bytes = include_bytes!("../../icons/tray-template.png");
        #[cfg(not(target_os = "macos"))]
        let bytes = include_bytes!("../../icons/32x32.png");

        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .expect("Failed to decode tray icon PNG");
        let info = reader.info().clone();
        let mut rgba = vec![0u8; reader.output_buffer_size()];
        reader
            .next_frame(&mut rgba)
            .expect("Failed to read tray icon frame");
        tauri::image::Image::new_owned(rgba, info.width, info.height)
    };

    let _tray = TrayIconBuilder::new()
        .tooltip("CodeTerm")
        .icon(tray_icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(w) = tray.app_handle().get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}
