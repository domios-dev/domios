use serde::Serialize;

use crate::client::{ResolutionSource, ResolvedSocket, RpcClient};

pub struct ResolvedValue<T> {
    pub value: T,
    pub source: ResolutionSource,
}

pub struct ResolvedApiConfig {
    pub data_dir: std::path::PathBuf,
    pub data_dir_source: ResolutionSource,
    pub host: String,
    pub host_source: ResolutionSource,
    pub port: i64,
    pub port_source: ResolutionSource,
    pub token: Option<String>,
}

pub fn resolved_data_root() -> ResolvedValue<std::path::PathBuf> {
    if let Ok(root) = std::env::var("CODETERM_DATA_DIR") {
        if !root.trim().is_empty() {
            let path = std::path::PathBuf::from(root);
            if path.is_absolute() {
                return ResolvedValue {
                    value: path,
                    source: ResolutionSource::CodeTermDataDir,
                };
            }
        }
    }
    let source = if resolved_instance().value.is_some() {
        ResolutionSource::CodeTermInstance
    } else if std::env::var("CODETERM_SOCKET_PATH")
        .map(|path| path.contains("codeterm-dev"))
        .unwrap_or(false)
    {
        ResolutionSource::CodeTermSocketPath
    } else if std::env::var("CODETERM_API_PORT").as_deref() == Ok("7686") {
        ResolutionSource::CodeTermApiPort
    } else {
        ResolutionSource::Default
    };
    ResolvedValue {
        value: dirs::home_dir()
            .unwrap_or_default()
            .join(crate::data_dir_name()),
        source,
    }
}

pub fn resolved_instance() -> ResolvedValue<Option<String>> {
    match std::env::var("CODETERM_INSTANCE") {
        Ok(value) if !value.trim().is_empty() => ResolvedValue {
            value: Some(value.trim().to_string()),
            source: ResolutionSource::CodeTermInstance,
        },
        _ => ResolvedValue {
            value: None,
            source: ResolutionSource::Absent,
        },
    }
}

pub fn resolve_api_config() -> ResolvedApiConfig {
    let resolved_data_dir = resolved_data_root();
    let config_dir = resolved_data_dir.value;
    let config_path = config_dir.join("config.toml");

    let (mut host, mut port) = ("127.0.0.1".to_string(), 7685i64);
    let mut host_source = ResolutionSource::Default;
    let mut port_source = ResolutionSource::Default;

    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(val) = content.parse::<toml::Table>() {
            if let Some(api) = val.get("api").and_then(|value| value.as_table()) {
                if let Some(raw_host) = api.get("host").and_then(|value| value.as_str()) {
                    host = if raw_host == "0.0.0.0" {
                        "127.0.0.1".to_string()
                    } else {
                        raw_host.to_string()
                    };
                    host_source = ResolutionSource::Config;
                }
                if let Some(config_port) = api.get("port").and_then(|value| value.as_integer()) {
                    port = config_port;
                    port_source = ResolutionSource::Config;
                }
            }
        }
    }

    if let Ok(env_port) = std::env::var("CODETERM_API_PORT") {
        if let Ok(parsed_port) = env_port.parse::<i64>() {
            port = parsed_port;
            port_source = ResolutionSource::CodeTermApiPort;
        }
    }

    let config_token = || {
        std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|content| content.parse::<toml::Table>().ok())
            .and_then(|value| {
                value
                    .get("api")?
                    .as_table()?
                    .get("token")?
                    .as_str()
                    .map(String::from)
            })
    };
    let signed_token = crate::read_mesh_signed_token();
    let env_token = std::env::var("CODETERM_API_TOKEN")
        .ok()
        .filter(|value| !value.is_empty());
    let targets_dev = std::env::var("CODETERM_INSTANCE")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || std::env::var("CODETERM_SOCKET_PATH")
            .map(|path| path.contains("codeterm-dev"))
            .unwrap_or(false)
        || std::env::var("CODETERM_API_PORT")
            .map(|value| !value.trim().is_empty() && value.trim() != "7685")
            .unwrap_or(false);
    let token = if targets_dev {
        config_token().or(signed_token).or(env_token)
    } else {
        env_token.or(signed_token).or_else(config_token)
    };

    ResolvedApiConfig {
        data_dir: config_dir,
        data_dir_source: resolved_data_dir.source,
        host,
        host_source,
        port,
        port_source,
        token,
    }
}

#[derive(Serialize)]
/// Stable machine-readable contract nested under `identify.connection`.
struct ConnectionIdentity {
    data_dir: String,
    data_dir_source: ResolutionSource,
    host: String,
    host_source: ResolutionSource,
    port: i64,
    port_source: ResolutionSource,
    socket_path: String,
    socket_source: ResolutionSource,
    instance: Option<String>,
    instance_source: ResolutionSource,
}

pub fn handle(
    client: &mut impl RpcClient,
    json: bool,
    socket: ResolvedSocket,
) -> Result<serde_json::Value, String> {
    let identity = client.call("system.identify", None)?;
    if !json {
        return Ok(identity);
    }

    let api = crate::resolve_api_config();
    let instance = crate::resolved_instance();
    extend_payload(
        identity,
        ConnectionIdentity {
            data_dir: api.data_dir.to_string_lossy().into_owned(),
            data_dir_source: api.data_dir_source,
            host: api.host,
            host_source: api.host_source,
            port: api.port,
            port_source: api.port_source,
            socket_path: socket.path,
            socket_source: socket.source,
            instance: instance.value,
            instance_source: instance.source,
        },
    )
}

fn extend_payload(
    mut identity: serde_json::Value,
    connection: ConnectionIdentity,
) -> Result<serde_json::Value, String> {
    let object = identity
        .as_object_mut()
        .ok_or_else(|| "system.identify returned a non-object payload".to_string())?;
    object.insert(
        "connection".to_string(),
        serde_json::to_value(connection).map_err(|error| error.to_string())?,
    );
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn restore_env(name: &str, value: Option<String>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    #[test]
    fn json_connection_shape_is_pinned() {
        let payload = extend_payload(
            serde_json::json!({
                "name": "CodeTerm",
                "version": "1.7.8",
                "platform": "windows",
                "features": []
            }),
            ConnectionIdentity {
                data_dir: "C:/data".to_string(),
                data_dir_source: ResolutionSource::CodeTermInstance,
                host: "127.0.0.1".to_string(),
                host_source: ResolutionSource::Config,
                port: 7788,
                port_source: ResolutionSource::CodeTermApiPort,
                socket_path: "codeterm-dev-bench.sock".to_string(),
                socket_source: ResolutionSource::CodeTermInstance,
                instance: Some("bench".to_string()),
                instance_source: ResolutionSource::CodeTermInstance,
            },
        )
        .unwrap();

        assert_eq!(
            payload["connection"],
            serde_json::json!({
                "data_dir": "C:/data",
                "data_dir_source": "CODETERM_INSTANCE",
                "host": "127.0.0.1",
                "host_source": "config",
                "port": 7788,
                "port_source": "CODETERM_API_PORT",
                "socket_path": "codeterm-dev-bench.sock",
                "socket_source": "CODETERM_INSTANCE",
                "instance": "bench",
                "instance_source": "CODETERM_INSTANCE"
            })
        );
    }

    #[test]
    fn explicit_port_override_reports_environment_source() {
        let _guard = crate::test_env_lock().lock().unwrap();
        let previous_data = std::env::var("CODETERM_DATA_DIR").ok();
        let previous_instance = std::env::var("CODETERM_INSTANCE").ok();
        let previous_port = std::env::var("CODETERM_API_PORT").ok();
        let data_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            data_dir.path().join("config.toml"),
            "[api]\nhost = \"0.0.0.0\"\nport = 7691\n",
        )
        .unwrap();
        std::env::set_var("CODETERM_DATA_DIR", data_dir.path());
        std::env::set_var("CODETERM_INSTANCE", "bench");
        std::env::set_var("CODETERM_API_PORT", "7788");

        let resolved = crate::resolve_api_config();
        let instance = crate::resolved_instance();
        assert_eq!(resolved.host, "127.0.0.1");
        assert_eq!(resolved.host_source, ResolutionSource::Config);
        assert_eq!(resolved.port, 7788);
        assert_eq!(resolved.port_source, ResolutionSource::CodeTermApiPort);
        assert_eq!(instance.value.as_deref(), Some("bench"));
        assert_eq!(instance.source, ResolutionSource::CodeTermInstance);

        restore_env("CODETERM_DATA_DIR", previous_data);
        restore_env("CODETERM_INSTANCE", previous_instance);
        restore_env("CODETERM_API_PORT", previous_port);
    }

    #[test]
    fn named_instance_reports_derived_data_dir() {
        let _guard = crate::test_env_lock().lock().unwrap();
        let previous_data = std::env::var("CODETERM_DATA_DIR").ok();
        let previous_instance = std::env::var("CODETERM_INSTANCE").ok();
        let previous_port = std::env::var("CODETERM_API_PORT").ok();
        std::env::remove_var("CODETERM_DATA_DIR");
        std::env::set_var("CODETERM_INSTANCE", "Bench Lane");
        std::env::set_var("CODETERM_API_PORT", "7691");

        let resolved = crate::resolved_data_root();
        assert!(resolved.value.ends_with(".codeterm-dev-bench-lane"));
        assert_eq!(resolved.source, ResolutionSource::CodeTermInstance);

        restore_env("CODETERM_DATA_DIR", previous_data);
        restore_env("CODETERM_INSTANCE", previous_instance);
        restore_env("CODETERM_API_PORT", previous_port);
    }

    #[test]
    fn default_dev_reports_absent_instance() {
        let _guard = crate::test_env_lock().lock().unwrap();
        let previous_data = std::env::var("CODETERM_DATA_DIR").ok();
        let previous_instance = std::env::var("CODETERM_INSTANCE").ok();
        let previous_port = std::env::var("CODETERM_API_PORT").ok();
        let previous_socket = std::env::var("CODETERM_SOCKET_PATH").ok();
        std::env::remove_var("CODETERM_DATA_DIR");
        std::env::remove_var("CODETERM_INSTANCE");
        std::env::set_var("CODETERM_API_PORT", "7686");
        std::env::remove_var("CODETERM_SOCKET_PATH");

        let data_dir = crate::resolved_data_root();
        let instance = crate::resolved_instance();
        assert!(data_dir.value.ends_with(".codeterm-dev"));
        assert_eq!(data_dir.source, ResolutionSource::CodeTermApiPort);
        assert_eq!(instance.value, None);
        assert_eq!(instance.source, ResolutionSource::Absent);

        restore_env("CODETERM_DATA_DIR", previous_data);
        restore_env("CODETERM_INSTANCE", previous_instance);
        restore_env("CODETERM_API_PORT", previous_port);
        restore_env("CODETERM_SOCKET_PATH", previous_socket);
    }
}
