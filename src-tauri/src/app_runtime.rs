#[cfg(not(test))]
pub type Runtime = tauri::Wry;
#[cfg(test)]
pub type Runtime = tauri::test::MockRuntime;

pub type AppHandle<R = Runtime> = tauri::AppHandle<R>;
pub type App<R = Runtime> = tauri::App<R>;
pub type Window<R = Runtime> = tauri::Window<R>;
pub type WebviewWindow<R = Runtime> = tauri::WebviewWindow<R>;

#[cfg(not(test))]
pub fn builder() -> tauri::Builder<Runtime> {
    tauri::Builder::default()
}

#[cfg(test)]
pub fn builder() -> tauri::Builder<Runtime> {
    tauri::test::mock_builder()
}
