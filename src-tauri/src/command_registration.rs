use codeterm_service_core::app_runtime::Runtime;

pub(crate) fn register_commands(builder: tauri::Builder<Runtime>) -> tauri::Builder<Runtime> {
    builder.invoke_handler(tauri::generate_handler![
        codeterm_commands::commands::get_windows_build_number,
        codeterm_commands::commands::materialize_dropped_files,
        codeterm_commands::clipboard_files::read_clipboard_files,
        codeterm_commands::clipboard_files::read_clipboard_text,
        codeterm_commands::commands::check_for_update,
        codeterm_commands::commands::install_app_update,
        codeterm_commands::commands::get_update_status,
        codeterm_commands::commands::open_folder,
        codeterm_commands::commands::open_url,
        codeterm_commands::commands::reveal_file,
        codeterm_commands::commands::notify_permission_state,
        codeterm_commands::commands::establish_notify_permission,
        codeterm_commands::commands::get_autostart_enabled,
        codeterm_commands::commands::set_autostart_enabled,
        codeterm_commands::commands::local_recording::local_recording_status,
        codeterm_commands::commands::local_recording::local_recording_set_enabled,
        codeterm_commands::commands::local_recording::local_recording_select_devices,
        codeterm_commands::commands::local_recording::local_recording_input_devices,
        codeterm_commands::commands::local_recording::local_recording_capture_stopped,
        codeterm_commands::commands::local_recording::local_recording_cancel,
        codeterm_commands::commands::local_recording::local_recording_manual_capture,
        codeterm_commands::commands::local_recording::local_recording_save_clip,
        codeterm_commands::commands::local_recording::local_recording_discard_clip,
        codeterm_commands::commands::local_recording::local_recording_commit,
    ])
}
