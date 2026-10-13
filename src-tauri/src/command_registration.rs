use crate::{clipboard_files, commands};

pub(crate) fn register_commands(
    builder: tauri::Builder<crate::app_runtime::Runtime>,
) -> tauri::Builder<crate::app_runtime::Runtime> {
    builder.invoke_handler(tauri::generate_handler![
        commands::get_windows_build_number,
        commands::agent_runtime::agent_runtime_retry,
        commands::agent_runtime::agent_runtime_answer_question,
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
        commands::local_recording::local_recording_capture_stopped,
        commands::local_recording::local_recording_cancel,
        commands::local_recording::local_recording_manual_capture,
        commands::local_recording::local_recording_save_clip,
        commands::local_recording::local_recording_discard_clip,
        commands::local_recording::local_recording_commit,
    ])
}
