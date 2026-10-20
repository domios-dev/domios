use super::probe::environment::{probe_path, Environment};

pub(crate) fn current_instance(
    api: codeterm_service_core::config::schema::ApiConfig,
) -> Environment {
    let instance = codeterm_service_core::service::instance_env::InstanceEnv::from_api_config(api);
    let cli = codeterm_service_core::service::instance_env::instance_cli_binary_path();
    let inherited = std::env::var_os("PATH");
    let path = probe_path(
        inherited.as_deref(),
        if cfg!(target_os = "macos") {
            &["/opt/homebrew/bin", "/usr/local/bin"]
        } else {
            &[]
        },
    );
    Environment::for_instance(
        &instance.socket_path,
        &instance.api_url,
        &instance.api_port,
        instance.api_token.as_deref(),
        cli.as_deref(),
        path.as_deref(),
    )
}
