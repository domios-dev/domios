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
    let instance = resolved_instance();
    let source = if instance.value.is_some() {
        instance.source
    } else if std::env::var("CODETERM_SOCKET_PATH")
        .map(|path| path.contains("domios-dev"))
        .unwrap_or(false)
    {
        ResolutionSource::CodeTermSocketPath
    } else if std::env::var("CODETERM_API_PORT")
        .map(|port| !port.trim().is_empty() && port.trim() != "7685")
        .unwrap_or(false)
    {
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
        _ => std::env::var("CODETERM_SOCKET_PATH")
            .ok()
            .and_then(|path| crate::client::instance_socket_name_from_path(&path))
            .map(|instance| ResolvedValue {
                value: Some(instance),
                source: ResolutionSource::CodeTermSocketPath,
            })
            .unwrap_or(ResolvedValue {
                value: None,
                source: ResolutionSource::Absent,
            }),
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

    let env_token = std::env::var("CODETERM_API_TOKEN")
        .ok()
        .filter(|value| !value.is_empty());
    let targets_dev = std::env::var("CODETERM_INSTANCE")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || std::env::var("CODETERM_SOCKET_PATH")
            .map(|path| path.contains("domios-dev"))
            .unwrap_or(false)
        || std::env::var("CODETERM_API_PORT")
            .map(|value| !value.trim().is_empty() && value.trim() != "7685")
            .unwrap_or(false);
    let require_stored = requires_stored_token(
        targets_dev,
        std::env::var("CODETERM_API_URL").ok().as_deref(),
        &host,
        port,
        port_source,
    );
    let token = resolve_connection_token(
        &config_dir, crate::nonempty_env("CODETERM_TAB_ID").as_deref(),
        env_token.as_deref(), require_stored,
    ).unwrap_or_else(|error| crate::cmd::print::fail(1, error));

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

fn resolve_connection_token(data_root: &std::path::Path, caller_tab: Option<&str>, environment: Option<&str>, require_stored: bool) -> Result<Option<String>, String> {
    if caller_tab.is_some() {
        if require_stored {
            return Err("A tab credential cannot authenticate another instance; use an owner shell on the target instance".into());
        }
        return environment.filter(|token| !token.is_empty()).map(|token| Some(token.to_owned()))
            .ok_or_else(|| "This tab has no launch credential; restart the tab".into());
    }
    codeterm_util::instance_token::resolve_at(data_root, environment, require_stored)
}

fn requires_stored_token(
    explicit_dev: bool,
    callback: Option<&str>,
    host: &str,
    port: i64,
    port_source: ResolutionSource,
) -> bool {
    if !explicit_dev {
        return false;
    }
    if port_source == ResolutionSource::Default {
        return true;
    }
    let loopback = |host: &str| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    };
    let matches = callback
        .and_then(|callback| reqwest::Url::parse(callback).ok())
        .is_some_and(|url| {
            url.scheme() == "http"
                && url.username().is_empty()
                && url.password().is_none()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none()
                && url.port_or_known_default().map(i64::from) == Some(port)
                && url.host_str().is_some_and(|callback_host| {
                    callback_host == host || (loopback(callback_host) && loopback(host))
                })
        });
    !matches
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

#[derive(Serialize)]
struct CliIdentity {
    version: &'static str,
    build_sha: &'static str,
}

pub fn handle(
    client: &mut impl RpcClient,
    json: bool,
    socket: ResolvedSocket,
) -> Result<serde_json::Value, String> {
    let identity = extend_cli_identity(client.call("system.identify", None)?)?;
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

fn extend_cli_identity(mut identity: serde_json::Value) -> Result<serde_json::Value, String> {
    let object = identity
        .as_object_mut()
        .ok_or_else(|| "system.identify returned a non-object payload".to_string())?;
    object.insert(
        "cli".to_string(),
        serde_json::to_value(CliIdentity {
            version: env!("CARGO_PKG_VERSION"),
            build_sha: env!("CODETERM_CLI_BUILD_SHA"),
        })
        .map_err(|error| error.to_string())?,
    );
    Ok(identity)
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
mod tests;
