mod command_registration;
mod host_ports;
mod plugin_bundle;
mod run;
mod startup;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    codeterm_build_info::install(env!("BUILD_SHA"), env!("BUILD_HASH"));
    codeterm_build_info::install_version(env!("CARGO_PKG_VERSION"));
    plugin_bundle::install();
    codeterm_orchestration::register_orchestration();
    run::run(
        tauri::generate_context!(),
        command_registration::register_commands,
    );
}

mod crash;
mod keepawake;
mod os_notify;

mod deep_links;
#[cfg(windows)]
mod native_resize;
mod webview_watchdog;

#[cfg(test)]
#[path = "../vendor/tauri-runtime-wry/src/owner_thread.rs"]
mod windows_rc_ownership_regression;
