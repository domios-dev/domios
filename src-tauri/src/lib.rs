mod command_registration;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    codeterm_build_info::install(env!("BUILD_SHA"), env!("BUILD_HASH"));
    codeterm_service_core::register_socket();
    codeterm_service_core::register_rest_hooks();
    codeterm_service_core::update::register_update_hooks();
    codeterm_service_core::sentinel::register_sentinel();
    codeterm_service_core::run(
        tauri::generate_context!(),
        command_registration::register_commands,
    );
}
