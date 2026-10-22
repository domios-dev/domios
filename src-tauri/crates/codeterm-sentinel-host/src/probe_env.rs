use super::probe::environment::Environment;

pub(crate) fn command() -> Environment {
    let cli = codeterm_service_core::service::instance_env::instance_cli_binary_path();
    Environment::for_command(
        std::env::vars_os(),
        cli.as_deref(),
        if cfg!(target_os = "macos") {
            &["/opt/homebrew/bin", "/usr/local/bin"]
        } else {
            &[]
        },
    )
}
