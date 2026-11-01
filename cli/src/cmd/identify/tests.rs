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
            "name": "Domios",
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
            socket_path: "domios-dev-bench.sock".to_string(),
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
            "socket_path": "domios-dev-bench.sock",
            "socket_source": "CODETERM_INSTANCE",
            "instance": "bench",
            "instance_source": "CODETERM_INSTANCE"
        })
    );
}

#[test]
fn cli_identity_includes_version_and_build_sha() {
    let payload = extend_cli_identity(serde_json::json!({
        "name": "Domios",
        "version": "1.7.8"
    }))
    .unwrap();

    assert_eq!(payload["cli"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(payload["cli"]["build_sha"], env!("CODETERM_CLI_BUILD_SHA"));
    assert_eq!(payload["cli"]["build_sha"].as_str().unwrap().len(), 40);
}

#[test]
fn explicit_port_override_reports_environment_source() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let previous_tab = std::env::var("CODETERM_TAB_ID").ok();
    std::env::remove_var("CODETERM_TAB_ID");
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

    restore_env("CODETERM_TAB_ID", previous_tab);
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
    assert!(resolved.value.ends_with(".domios-dev-bench-lane"));
    assert_eq!(resolved.source, ResolutionSource::CodeTermInstance);

    restore_env("CODETERM_DATA_DIR", previous_data);
    restore_env("CODETERM_INSTANCE", previous_instance);
    restore_env("CODETERM_API_PORT", previous_port);
}

#[test]
fn named_socket_selects_its_data_dir_without_instance_env() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let previous_data = std::env::var("CODETERM_DATA_DIR").ok();
    let previous_instance = std::env::var("CODETERM_INSTANCE").ok();
    let previous_port = std::env::var("CODETERM_API_PORT").ok();
    let previous_socket = std::env::var("CODETERM_SOCKET_PATH").ok();
    std::env::remove_var("CODETERM_DATA_DIR");
    std::env::remove_var("CODETERM_INSTANCE");
    std::env::set_var("CODETERM_API_PORT", "7712");
    std::env::set_var("CODETERM_SOCKET_PATH", "domios-dev-coreqa.sock");

    let data_dir = crate::resolved_data_root();
    let instance = resolved_instance();
    assert!(data_dir.value.ends_with(".domios-dev-coreqa"));
    assert_eq!(data_dir.source, ResolutionSource::CodeTermSocketPath);
    assert_eq!(instance.value.as_deref(), Some("coreqa"));
    assert_eq!(instance.source, ResolutionSource::CodeTermSocketPath);

    restore_env("CODETERM_DATA_DIR", previous_data);
    restore_env("CODETERM_INSTANCE", previous_instance);
    restore_env("CODETERM_API_PORT", previous_port);
    restore_env("CODETERM_SOCKET_PATH", previous_socket);
}

#[test]
fn custom_dev_port_does_not_select_the_production_data_dir() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let previous_data = std::env::var("CODETERM_DATA_DIR").ok();
    let previous_instance = std::env::var("CODETERM_INSTANCE").ok();
    let previous_port = std::env::var("CODETERM_API_PORT").ok();
    let previous_socket = std::env::var("CODETERM_SOCKET_PATH").ok();
    std::env::remove_var("CODETERM_DATA_DIR");
    std::env::remove_var("CODETERM_INSTANCE");
    std::env::set_var("CODETERM_API_PORT", "7712");
    std::env::remove_var("CODETERM_SOCKET_PATH");

    let data_dir = crate::resolved_data_root();
    assert!(data_dir.value.ends_with(".domios-dev"));
    assert_eq!(data_dir.source, ResolutionSource::CodeTermApiPort);

    restore_env("CODETERM_DATA_DIR", previous_data);
    restore_env("CODETERM_INSTANCE", previous_instance);
    restore_env("CODETERM_API_PORT", previous_port);
    restore_env("CODETERM_SOCKET_PATH", previous_socket);
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
    assert!(data_dir.value.ends_with(".domios-dev"));
    assert_eq!(data_dir.source, ResolutionSource::CodeTermApiPort);
    assert_eq!(instance.value, None);
    assert_eq!(instance.source, ResolutionSource::Absent);

    restore_env("CODETERM_DATA_DIR", previous_data);
    restore_env("CODETERM_INSTANCE", previous_instance);
    restore_env("CODETERM_API_PORT", previous_port);
    restore_env("CODETERM_SOCKET_PATH", previous_socket);
}

#[test]
fn dev_tab_environment_is_usable_without_reading_the_secret_store() {
    assert!(!requires_stored_token(
        true,
        Some("http://127.0.0.1:7686"),
        "127.0.0.1",
        7686,
        ResolutionSource::CodeTermApiPort
    ));
    assert!(!requires_stored_token(
        true,
        Some("http://127.0.0.1:7686"),
        "localhost",
        7686,
        ResolutionSource::CodeTermApiPort
    ));
    assert!(requires_stored_token(
        true,
        Some("http://127.0.0.1:7685"),
        "127.0.0.1",
        7686,
        ResolutionSource::CodeTermApiPort
    ));
    assert!(requires_stored_token(
        true,
        None,
        "127.0.0.1",
        7686,
        ResolutionSource::CodeTermApiPort
    ));
    assert!(requires_stored_token(
        true,
        Some("http://other.example:7686"),
        "127.0.0.1",
        7686,
        ResolutionSource::CodeTermApiPort
    ));
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".secrets"), "unreadable").unwrap();
    assert_eq!(
        codeterm_util::instance_token::resolve_at(root.path(), Some("tab-scoped"), false)
            .unwrap()
            .as_deref(),
        Some("tab-scoped")
    );
}

#[test]
fn explicit_dev_with_default_port_cannot_inherit_production_authority() {
    let root = tempfile::tempdir().unwrap();
    codeterm_util::secret_file::SecretFile::new_at(root.path(), "domios.instance")
        .set("api_token", "selected-dev")
        .unwrap();
    let require_stored = requires_stored_token(
        true,
        Some("http://127.0.0.1:7685"),
        "127.0.0.1",
        7685,
        ResolutionSource::Default,
    );
    assert!(require_stored);
    assert_eq!(
        codeterm_util::instance_token::resolve_at(
            root.path(),
            Some("inherited-prod"),
            require_stored
        )
        .unwrap()
        .as_deref(),
        Some("selected-dev"),
    );
}

#[test]
fn tab_connections_never_read_owner_authority_or_send_credentials_to_a_foreign_selection() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".secrets"), "unreadable fixture").unwrap();
    assert_eq!(
        resolve_connection_token(root.path(), Some("tab"), Some("launch-token"), false)
            .unwrap()
            .as_deref(),
        Some("launch-token")
    );
    assert!(
        resolve_connection_token(root.path(), Some("tab"), None, false)
            .unwrap_err()
            .contains("no launch credential")
    );
    assert!(
        resolve_connection_token(root.path(), Some("tab"), Some("launch-token"), true)
            .unwrap_err()
            .contains("another instance")
    );
}
