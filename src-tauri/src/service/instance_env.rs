use std::path::PathBuf;

pub(crate) struct InstanceEnv {
    pub socket_path: String,
    pub api_url: String,
    pub api_port: String,
    pub api_token: Option<String>,
    pub cli_path: Option<PathBuf>,
}

impl InstanceEnv {
    pub(crate) fn resolve() -> Self {
        let api = crate::config::load_config().api;
        let (api_url, api_port) = crate::config::api_callback_url(&api);
        Self {
            socket_path: crate::socket::server::socket_display_path(),
            api_url,
            api_port,
            api_token: api.token,
            cli_path: instance_cli_binary_path(),
        }
    }
}

pub(crate) fn instance_cli_binary_path() -> Option<PathBuf> {
    let local_cli = crate::paths::data_dir()
        .join("bin")
        .join(format!("codeterm{}", std::env::consts::EXE_SUFFIX));
    if local_cli.exists() {
        Some(local_cli)
    } else if !crate::paths::is_dev_instance() {
        std::env::var_os("CODETERM_CLI_PATH").map(PathBuf::from)
    } else {
        None
    }
}
