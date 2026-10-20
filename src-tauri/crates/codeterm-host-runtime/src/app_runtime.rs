#[cfg(not(any(test, feature = "test-support")))]
pub type Runtime = tauri::Wry;
#[cfg(any(test, feature = "test-support"))]
pub type Runtime = tauri::test::MockRuntime;

pub type AppHandle<R = Runtime> = tauri::AppHandle<R>;
pub type App<R = Runtime> = tauri::App<R>;
pub type Window<R = Runtime> = tauri::Window<R>;
pub type WebviewWindow<R = Runtime> = tauri::WebviewWindow<R>;

#[cfg(not(any(test, feature = "test-support")))]
pub fn builder() -> tauri::Builder<Runtime> {
    tauri::Builder::default()
}

#[cfg(any(test, feature = "test-support"))]
pub fn builder() -> tauri::Builder<Runtime> {
    tauri::test::mock_builder()
}
