//! Cross-platform window screenshot capture.
//!
//! Captures the CodeTerm window even when behind other windows.
//! Returns compressed JPEG (small size, suitable for LLM analysis).
//!
//! Platform support:
//! - Windows: PrintWindow API (captures occluded windows)
//! - macOS: screencapture -l <window_id>
//! - Linux: xdotool + import (X11) / fallback to grim (Wayland)

use std::sync::Arc;
use parking_lot::Mutex;
use tauri::AppHandle;

/// Shared state — kept for API/Tauri state management.
pub struct ScreenshotChannel;

impl ScreenshotChannel {
    pub fn new() -> Self { Self }
}

pub type ScreenshotState = Arc<Mutex<ScreenshotChannel>>;

/// Max width for screenshots (resized to fit, keeps aspect ratio).
const MAX_WIDTH: u32 = 1280;
/// JPEG quality (1-100). 65 gives good readability at ~200-400KB.
const JPEG_QUALITY: u8 = 65;

/// Capture the CodeTerm window and return base64-encoded JPEG.
/// Works even when the window is behind other windows (on supported platforms).
pub fn capture_window(app: &AppHandle) -> Result<CaptureResult, String> {
    let raw = platform_capture(app)?;

    // Decode raw RGBA into image
    let img = image::RgbaImage::from_raw(raw.width, raw.height, raw.pixels)
        .ok_or("Failed to create image from pixel data")?;
    let img = image::DynamicImage::ImageRgba8(img);

    // Resize if too wide
    let img = if img.width() > MAX_WIDTH {
        img.resize(MAX_WIDTH, u32::MAX, image::imageops::FilterType::Triangle)
    } else {
        img
    };

    // Encode as JPEG
    let mut jpeg_buf: Vec<u8> = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut jpeg_buf);
    img.write_to(&mut cursor, image::ImageFormat::Jpeg)
        .map_err(|e| format!("JPEG encode failed: {}", e))?;

    // Base64
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_buf);

    Ok(CaptureResult {
        data_url: format!("data:image/jpeg;base64,{}", b64),
        width: img.width(),
        height: img.height(),
        size_bytes: jpeg_buf.len(),
    })
}

pub struct CaptureResult {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    pub size_bytes: usize,
}

struct RawCapture {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

/// Tauri command for screenshot capture.
#[tauri::command]
pub async fn capture_screenshot(
    app: AppHandle,
    _state: tauri::State<'_, ScreenshotState>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let result = capture_window(&app)?;
        Ok(serde_json::json!({
            "screenshot": result.data_url,
            "width": result.width,
            "height": result.height,
            "size_kb": result.size_bytes / 1024,
        }).to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Legacy command kept for API compatibility.
#[tauri::command]
pub fn submit_screenshot(
    _state: tauri::State<'_, ScreenshotState>,
    _data: String,
) -> Result<(), String> {
    Ok(())
}

/// Legacy request function — now delegates to capture_window.
pub fn request_screenshot(
    app: &AppHandle,
    _screenshot_state: &ScreenshotState,
    _timeout_ms: u64,
) -> Result<String, String> {
    let result = capture_window(app)?;
    Ok(result.data_url)
}

// ── Platform implementations ────────────────────────────────────────

fn platform_capture(app: &AppHandle) -> Result<RawCapture, String> {
    #[cfg(target_os = "windows")]
    { capture_windows(app) }
    #[cfg(target_os = "macos")]
    { capture_macos(app) }
    #[cfg(target_os = "linux")]
    { capture_linux(app) }
}

// ── Windows: PrintWindow API ────────────────────────────────────────

#[cfg(target_os = "windows")]
fn capture_windows(app: &AppHandle) -> Result<RawCapture, String> {
    use tauri::Manager;
    use std::ffi::c_void;

    let window = app.get_webview_window("main")
        .ok_or("Main window not found")?;
    let hwnd = window.hwnd().map_err(|e| format!("HWND: {}", e))?.0 as isize;

    unsafe { capture_hwnd_printwindow(hwnd) }
}

#[cfg(target_os = "windows")]
unsafe fn capture_hwnd_printwindow(hwnd: isize) -> Result<RawCapture, String> {
    use std::ffi::c_void;

    #[repr(C)]
    struct RECT { left: i32, top: i32, right: i32, bottom: i32 }

    #[repr(C, packed)]
    #[allow(non_snake_case)]
    struct BITMAPINFOHEADER {
        biSize: u32, biWidth: i32, biHeight: i32, biPlanes: u16,
        biBitCount: u16, biCompression: u32, biSizeImage: u32,
        biXPelsPerMeter: i32, biYPelsPerMeter: i32,
        biClrUsed: u32, biClrImportant: u32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetWindowRect(hwnd: isize, rect: *mut RECT) -> i32;
        fn GetDC(hwnd: isize) -> isize;
        fn ReleaseDC(hwnd: isize, hdc: isize) -> i32;
        fn PrintWindow(hwnd: isize, hdc: isize, flags: u32) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateCompatibleDC(hdc: isize) -> isize;
        fn CreateCompatibleBitmap(hdc: isize, w: i32, h: i32) -> isize;
        fn SelectObject(hdc: isize, obj: isize) -> isize;
        fn DeleteObject(obj: isize) -> i32;
        fn DeleteDC(hdc: isize) -> i32;
        fn GetDIBits(hdc: isize, bmp: isize, start: u32, lines: u32,
                     bits: *mut c_void, bi: *mut BITMAPINFOHEADER, usage: u32) -> i32;
    }

    // Get window dimensions
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetWindowRect(hwnd, &mut rect);
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return Err(format!("Invalid window size: {}x{}", width, height));
    }

    // Create memory DC + bitmap
    let hdc_window = GetDC(hwnd);
    let hdc_mem = CreateCompatibleDC(hdc_window);
    let hbmp = CreateCompatibleBitmap(hdc_window, width, height);
    let old_bmp = SelectObject(hdc_mem, hbmp);

    // PrintWindow: PW_RENDERFULLCONTENT (0x02) captures DWM/WebView2 content
    // even when window is behind other windows
    let result = PrintWindow(hwnd, hdc_mem, 0x02);
    if result == 0 {
        // Fallback to basic PrintWindow without PW_RENDERFULLCONTENT
        PrintWindow(hwnd, hdc_mem, 0);
    }

    // Read pixel data as BGRA
    let mut bi = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width, biHeight: -height, // top-down
        biPlanes: 1, biBitCount: 32, biCompression: 0,
        biSizeImage: 0, biXPelsPerMeter: 0, biYPelsPerMeter: 0,
        biClrUsed: 0, biClrImportant: 0,
    };

    let pixel_count = (width * height) as usize;
    let mut pixels = vec![0u8; pixel_count * 4];
    GetDIBits(hdc_mem, hbmp, 0, height as u32,
              pixels.as_mut_ptr() as *mut c_void, &mut bi, 0);

    // Cleanup
    SelectObject(hdc_mem, old_bmp);
    DeleteObject(hbmp);
    DeleteDC(hdc_mem);
    ReleaseDC(hwnd, hdc_window);

    // BGRA → RGBA
    for i in 0..pixel_count {
        pixels.swap(i * 4, i * 4 + 2);
    }

    Ok(RawCapture { pixels, width: width as u32, height: height as u32 })
}

// ── macOS: screencapture CLI ────────────────────────────────────────

#[cfg(target_os = "macos")]
fn capture_macos(app: &AppHandle) -> Result<RawCapture, String> {
    use tauri::Manager;

    // Get window number for screencapture -l flag
    let window = app.get_webview_window("main")
        .ok_or("Main window not found")?;

    // On macOS, we use CGWindowID. Tauri provides ns_window.
    // Get CGWindowID via NSWindow.windowNumber
    let ns_window = window.ns_window()
        .map_err(|e| format!("NSWindow: {}", e))?;

    // NSWindow.windowNumber is an NSInteger — get it via objc
    let window_id: i64 = unsafe {
        let ptr = ns_window.0 as *const std::ffi::c_void;
        // objc_msgSend(ns_window, sel_registerName("windowNumber"))
        type MsgSendFn = unsafe extern "C" fn(*const std::ffi::c_void, *const std::ffi::c_void) -> i64;
        #[link(name = "objc")]
        extern "C" {
            fn sel_registerName(name: *const u8) -> *const std::ffi::c_void;
            fn objc_msgSend();
        }
        let sel = sel_registerName(b"windowNumber\0".as_ptr());
        let msg_send: MsgSendFn = std::mem::transmute(objc_msgSend as *const ());
        msg_send(ptr, sel)
    };

    // screencapture -l <windowid> -x -t png <path>
    let tmp = std::env::temp_dir().join(format!("codeterm-cap-{}.png", std::process::id()));
    let output = std::process::Command::new("screencapture")
        .args(["-l", &window_id.to_string(), "-x", "-t", "png", tmp.to_str().unwrap()])
        .output()
        .map_err(|e| format!("screencapture failed: {}", e))?;

    if !output.status.success() {
        return Err(format!("screencapture exit code: {}", output.status));
    }

    // Read PNG, decode to RGBA
    let png_bytes = std::fs::read(&tmp).map_err(|e| format!("Read: {}", e))?;
    let _ = std::fs::remove_file(&tmp);

    let img = image::load_from_memory(&png_bytes)
        .map_err(|e| format!("Decode: {}", e))?;
    let rgba = img.to_rgba8();

    Ok(RawCapture {
        width: rgba.width(),
        height: rgba.height(),
        pixels: rgba.into_raw(),
    })
}

// ── Linux: xdotool + import / grim ─────────────────────────────────

#[cfg(target_os = "linux")]
fn capture_linux(_app: &AppHandle) -> Result<RawCapture, String> {
    // Try X11 first: find CodeTerm window, capture with import
    let tmp = std::env::temp_dir().join(format!("codeterm-cap-{}.png", std::process::id()));
    let tmp_str = tmp.to_str().unwrap();

    // Try xdotool to find window by name
    let wid = std::process::Command::new("xdotool")
        .args(["search", "--name", "CodeTerm"])
        .output();

    if let Ok(output) = wid {
        if output.status.success() {
            let id = String::from_utf8_lossy(&output.stdout)
                .lines().next().unwrap_or("").trim().to_string();
            if !id.is_empty() {
                // import -window <id> <path>
                let result = std::process::Command::new("import")
                    .args(["-window", &id, tmp_str])
                    .output();
                if let Ok(r) = result {
                    if r.status.success() {
                        return load_png_file(&tmp);
                    }
                }
            }
        }
    }

    // Wayland fallback: grim
    let result = std::process::Command::new("grim")
        .args(["-t", "png", tmp_str])
        .output();
    if let Ok(r) = result {
        if r.status.success() {
            return load_png_file(&tmp);
        }
    }

    Err("No screenshot tool available (tried xdotool+import, grim)".to_string())
}

#[cfg(target_os = "linux")]
fn load_png_file(path: &std::path::Path) -> Result<RawCapture, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Read: {}", e))?;
    let _ = std::fs::remove_file(path);
    let img = image::load_from_memory(&bytes).map_err(|e| format!("Decode: {}", e))?;
    let rgba = img.to_rgba8();
    Ok(RawCapture {
        width: rgba.width(),
        height: rgba.height(),
        pixels: rgba.into_raw(),
    })
}
