use crate::{clipboard_files, commands};

pub(crate) fn register_commands(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        commands::get_windows_build_number,
        commands::materialize_dropped_files,
        clipboard_files::read_clipboard_files,
        clipboard_files::read_clipboard_text,
        commands::check_for_update,
        commands::install_app_update,
        commands::get_update_status,
        commands::open_folder,
        commands::open_url,
        commands::reveal_file,
        commands::notify_permission_state,
        commands::establish_notify_permission,
        commands::get_autostart_enabled,
        commands::set_autostart_enabled,
        commands::local_recording::local_recording_status,
        commands::local_recording::local_recording_set_enabled,
        commands::local_recording::local_recording_select_devices,
        commands::local_recording::local_recording_input_devices,
        commands::local_recording::local_recording_battery,
        commands::local_recording::local_recording_capture_stopped,
        commands::local_recording::local_recording_cancel,
        commands::local_recording::local_recording_manual_capture,
        commands::local_recording::local_recording_save_clip,
        commands::local_recording::local_recording_discard_clip,
        commands::local_recording::local_recording_commit,
    ])
}
