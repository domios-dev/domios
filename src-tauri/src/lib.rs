mod command_registration;
mod run;
mod startup;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    codeterm_build_info::install(env!("BUILD_SHA"), env!("BUILD_HASH"));
    codeterm_socket::register_socket();
    codeterm_service_core::register_orchestration();
    run::run(
        tauri::generate_context!(),
        command_registration::register_commands,
    );
}
