use std::path::PathBuf;

pub struct InstanceEnv {
    pub socket_path: String,
    pub api_url: String,
    pub api_port: String,
    pub cli_path: Option<PathBuf>,
}

impl InstanceEnv {
    pub fn from_api_config(api: crate::config::schema::ApiConfig) -> Self {
        let (api_url, api_port) = crate::config::api_callback_url(&api);
        Self {
            socket_path: crate::paths::socket_display_path(),
            api_url,
            api_port,
            cli_path: instance_cli_binary_path(),
        }
    }
}

pub fn instance_cli_binary_path() -> Option<PathBuf> {
    select_instance_cli_path(
        std::env::var_os("CODETERM_CLI_PATH").map(PathBuf::from),
        || {
            let local_cli = crate::paths::data_dir()
                .join("bin")
                .join(format!("codeterm{}", std::env::consts::EXE_SUFFIX));
            local_cli.exists().then_some(local_cli)
        },
    )
}

fn select_instance_cli_path(
    lifecycle_pin: Option<PathBuf>,
    local_cli: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    lifecycle_pin.or_else(local_cli)
}

#[cfg(test)]
mod tests;
