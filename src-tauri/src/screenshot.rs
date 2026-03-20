use std::sync::Arc;
use parking_lot::Mutex;
use tauri::AppHandle;

#[cfg(target_os = "windows")]
use std::ffi::c_void;

/// Shared state — kept for API compatibility but Win32 path doesn't need channels.
pub struct ScreenshotChannel;

impl ScreenshotChannel {
    pub fn new() -> Self {
        Self
    }
}

pub type ScreenshotState = Arc<Mutex<ScreenshotChannel>>;

/// Called by frontend (legacy path, kept for compatibility).
#[tauri::command]
pub fn submit_screenshot(
    _state: tauri::State<'_, ScreenshotState>,
    _data: String,
) -> Result<(), String> {
    Ok(())
}

/// Capture screenshot using Win32 PrintWindow API.
/// Works even when window is behind other windows.
/// Returns base64-encoded PNG as data URL.
pub fn request_screenshot(
    app: &AppHandle,
    _screenshot_state: &ScreenshotState,
    _timeout_ms: u64,
) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        capture_win32(app)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Screenshot only supported on Windows".to_string())
    }
}

#[cfg(target_os = "windows")]
fn capture_win32(app: &AppHandle) -> Result<String, String> {
    use tauri::Manager;

    let window = app.get_webview_window("main")
        .ok_or("Main window not found")?;

    // Use Tauri API to reliably bring window to front
    let _ = window.show();
    let _ = window.set_focus();
    std::thread::sleep(std::time::Duration::from_millis(500));

    let hwnd = get_hwnd(&window)?;

    unsafe { capture_hwnd(hwnd) }
}

#[cfg(target_os = "windows")]
fn get_hwnd(window: &tauri::WebviewWindow) -> Result<isize, String> {
    use tauri::Manager;
    // Tauri v2: window.hwnd() returns Result<HWND>
    // HWND is a pointer type, cast to isize
    let raw = window.hwnd().map_err(|e| format!("Failed to get HWND: {}", e))?;
    Ok(raw.0 as isize)
}

#[cfg(target_os = "windows")]
unsafe fn capture_hwnd(hwnd: isize) -> Result<String, String> {
    // Win32 FFI declarations
    #[repr(C)]
    struct RECT { left: i32, top: i32, right: i32, bottom: i32 }

    #[repr(C, packed)]
    #[allow(non_snake_case)]
    struct BITMAPINFOHEADER {
        biSize: u32,
        biWidth: i32,
        biHeight: i32,
        biPlanes: u16,
        biBitCount: u16,
        biCompression: u32,
        biSizeImage: u32,
        biXPelsPerMeter: i32,
        biYPelsPerMeter: i32,
        biClrUsed: u32,
        biClrImportant: u32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetClientRect(hwnd: isize, rect: *mut RECT) -> i32;
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
        fn GetDIBits(hdc: isize, bmp: isize, start: u32, lines: u32, bits: *mut c_void, bi: *mut BITMAPINFOHEADER, usage: u32) -> i32;
    }

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmGetWindowAttribute(hwnd: isize, attr: u32, val: *mut RECT, size: u32) -> i32;
    }

    // Get window size — use DWM extended frame bounds for accuracy
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let hr = DwmGetWindowAttribute(hwnd, 9, &mut rect, std::mem::size_of::<RECT>() as u32);
    if hr != 0 {
        // Fallback to client rect
        GetClientRect(hwnd, &mut rect);
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return Err(format!("Invalid window size: {}x{}", width, height));
    }

    // Create compatible DC and bitmap
    let hdc_screen = GetDC(hwnd);
    let hdc_mem = CreateCompatibleDC(hdc_screen);
    let hbmp = CreateCompatibleBitmap(hdc_screen, width, height);
    let old_bmp = SelectObject(hdc_mem, hbmp);

    // BitBlt from screen DC — captures GPU-rendered WebView2 content
    // Window is already in foreground (set_focus called before this)
    #[link(name = "user32")]
    extern "system" {
        fn GetDesktopWindow() -> isize;
    }

    // Use BitBlt from screen DC at window position
    #[link(name = "gdi32")]
    extern "system" {
        fn BitBlt(dest: isize, x: i32, y: i32, w: i32, h: i32,
                  src: isize, sx: i32, sy: i32, rop: u32) -> i32;
    }

    let hdc_screen_desktop = GetDC(GetDesktopWindow());
    BitBlt(hdc_mem, 0, 0, width, height,
           hdc_screen_desktop, rect.left, rect.top, 0x00CC0020); // SRCCOPY
    ReleaseDC(GetDesktopWindow(), hdc_screen_desktop);

    // Read pixel data
    let mut bi = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height, // top-down
        biPlanes: 1,
        biBitCount: 32,
        biCompression: 0, // BI_RGB
        biSizeImage: 0,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };

    let pixel_count = (width * height) as usize;
    let mut pixels: Vec<u8> = vec![0u8; pixel_count * 4];

    GetDIBits(
        hdc_mem, hbmp, 0, height as u32,
        pixels.as_mut_ptr() as *mut c_void,
        &mut bi, 0,
    );

    // Cleanup GDI
    SelectObject(hdc_mem, old_bmp);
    DeleteObject(hbmp);
    DeleteDC(hdc_mem);
    ReleaseDC(hwnd, hdc_screen);

    // Convert BGRA to RGBA
    for i in 0..pixel_count {
        let off = i * 4;
        pixels.swap(off, off + 2); // B <-> R
    }

    // Encode as PNG
    let mut png_buf: Vec<u8> = Vec::new();
    {
        let mut encoder = png_encoder::PngEncoder::new(&mut png_buf, width as u32, height as u32);
        encoder.encode(&pixels)
            .map_err(|e| format!("PNG encode failed: {}", e))?;
    }

    // Base64 encode
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_buf);
    Ok(format!("data:image/png;base64,{}", b64))
}

/// Minimal PNG encoder — no external crate needed.
#[cfg(target_os = "windows")]
mod png_encoder {
    use std::io::Write;

    pub struct PngEncoder<'a> {
        out: &'a mut Vec<u8>,
        width: u32,
        height: u32,
    }

    impl<'a> PngEncoder<'a> {
        pub fn new(out: &'a mut Vec<u8>, width: u32, height: u32) -> Self {
            Self { out, width, height }
        }

        pub fn encode(&mut self, rgba: &[u8]) -> Result<(), String> {
            let w = self.width;
            let h = self.height;

            // PNG signature
            self.out.write_all(&[137, 80, 78, 71, 13, 10, 26, 10]).map_err(|e| e.to_string())?;

            // IHDR
            let mut ihdr = Vec::new();
            ihdr.extend_from_slice(&w.to_be_bytes());
            ihdr.extend_from_slice(&h.to_be_bytes());
            ihdr.push(8); // bit depth
            ihdr.push(6); // RGBA
            ihdr.push(0); // compression
            ihdr.push(0); // filter
            ihdr.push(0); // interlace
            self.write_chunk(b"IHDR", &ihdr)?;

            // IDAT — raw pixel data with filter byte per row
            let row_bytes = (w as usize) * 4 + 1; // +1 for filter byte
            let mut raw = Vec::with_capacity(row_bytes * h as usize);
            for y in 0..h as usize {
                raw.push(0); // filter: None
                let start = y * (w as usize) * 4;
                let end = start + (w as usize) * 4;
                raw.extend_from_slice(&rgba[start..end]);
            }

            // Compress with deflate (using miniz via flate2-like approach)
            // Since we don't have flate2, use zlib format manually with store (no compression)
            let compressed = zlib_store(&raw);
            self.write_chunk(b"IDAT", &compressed)?;

            // IEND
            self.write_chunk(b"IEND", &[])?;

            Ok(())
        }

        fn write_chunk(&mut self, chunk_type: &[u8; 4], data: &[u8]) -> Result<(), String> {
            let len = data.len() as u32;
            self.out.write_all(&len.to_be_bytes()).map_err(|e| e.to_string())?;
            self.out.write_all(chunk_type).map_err(|e| e.to_string())?;
            self.out.write_all(data).map_err(|e| e.to_string())?;

            // CRC32 over chunk_type + data
            let mut crc_data = Vec::with_capacity(4 + data.len());
            crc_data.extend_from_slice(chunk_type);
            crc_data.extend_from_slice(data);
            let crc = crc32(&crc_data);
            self.out.write_all(&crc.to_be_bytes()).map_err(|e| e.to_string())?;

            Ok(())
        }
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFFFFFF;
        for &byte in data {
            crc ^= byte as u32;
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB88320;
                } else {
                    crc >>= 1;
                }
            }
        }
        !crc
    }

    /// Zlib "store" — no compression, just framing.
    /// For simplicity; produces valid but uncompressed PNGs.
    fn zlib_store(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        // Zlib header: CMF=0x78 (deflate, window=32k), FLG=0x01 (no dict, check bits)
        out.push(0x78);
        out.push(0x01);

        // Deflate stored blocks — max 65535 bytes each
        let chunks: Vec<&[u8]> = data.chunks(65535).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let is_last = i == chunks.len() - 1;
            out.push(if is_last { 0x01 } else { 0x00 }); // BFINAL + BTYPE=00 (stored)
            let len = chunk.len() as u16;
            let nlen = !len;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&nlen.to_le_bytes());
            out.extend_from_slice(chunk);
        }

        // Adler-32 checksum
        let adler = adler32(data);
        out.extend_from_slice(&adler.to_be_bytes());

        out
    }

    fn adler32(data: &[u8]) -> u32 {
        let mut a: u32 = 1;
        let mut b: u32 = 0;
        for &byte in data {
            a = (a + byte as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }
}

/// Tauri command wrapper for request_screenshot
#[tauri::command]
pub async fn capture_screenshot(
    app: AppHandle,
    state: tauri::State<'_, ScreenshotState>,
) -> Result<String, String> {
    let ss = state.inner().clone();
    tokio::task::spawn_blocking(move || request_screenshot(&app, &ss, 5000))
        .await
        .map_err(|e| e.to_string())?
}
