use codeterm_client_shell::app_runtime::Runtime;

pub(crate) fn register_commands(builder: tauri::Builder<Runtime>) -> tauri::Builder<Runtime> {
    builder.invoke_handler(tauri::generate_handler![
        crate::host_ports::frontend_view_root_ready,
        crate::host_ports::native_webview_diagnostics_snapshot,
        crate::os_notify::notify_permission_state,
        crate::os_notify::establish_notify_permission,
        codeterm_client_shell::commands::get_windows_build_number,
        codeterm_client_shell::commands::materialize_dropped_files,
        codeterm_client_shell::clipboard_files::read_clipboard_files,
        codeterm_client_shell::clipboard_files::read_clipboard_text,
        codeterm_client_shell::commands::check_for_update,
        codeterm_client_shell::commands::install_app_update,
        codeterm_client_shell::commands::get_update_status,
        codeterm_client_shell::commands::open_folder,
        codeterm_client_shell::commands::open_url,
        codeterm_client_shell::commands::reveal_file,
        codeterm_client_shell::commands::get_autostart_enabled,
        codeterm_client_shell::commands::set_autostart_enabled,
    ])
}
