use super::{InstanceSelector, LaunchOptions, ProcessProvenance};
use std::{net::SocketAddr, path::PathBuf};

#[test]
fn development_launch_ownership_is_explicit_and_never_admitted_for_production() {
    let mut options = LaunchOptions {
        instance: InstanceSelector::Development {
            instance: Some("fixture".into()),
            port: 17996,
        },
        resources: PathBuf::from("resources"),
        probe_sink: None,
        probe_identity_alias: None,
        development_owner: None,
    };
    assert!(serde_json::to_value(&options)
        .unwrap()
        .get("development_owner")
        .is_none());
    options.development_owner = Some(ProcessProvenance {
        pid: 42,
        created: "opaque".into(),
        executable: PathBuf::from("client"),
    });
    assert!(options.validate(true).is_ok());
    assert!(options.validate(false).is_err());
    options.instance = InstanceSelector::Production;
    assert!(options.validate(false).is_err());
    assert!(options.validate(true).is_err());
}

fn isolated_options(data_dir: PathBuf, config_dir: PathBuf, bind: SocketAddr) -> LaunchOptions {
    LaunchOptions {
        instance: InstanceSelector::IsolatedVerification {
            instance: None,
            data_dir,
            config_dir,
            bind,
        },
        resources: PathBuf::from("resources"),
        probe_sink: None,
        probe_identity_alias: None,
        development_owner: None,
    }
}

#[test]
fn server_bootstrap_isolated_verification_accepts_both_artifact_profiles() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let mut options = isolated_options(
        data.path().to_owned(),
        config.path().to_owned(),
        "127.0.0.1:18765".parse().unwrap(),
    );

    assert!(options.validate(false).is_ok());
    assert!(options.validate(true).is_err());
    if let InstanceSelector::IsolatedVerification { instance, .. } = &mut options.instance {
        *instance = Some("verification-a".into());
    }
    assert!(options.validate(true).is_ok());
    assert!(options.validate(false).is_err());
}

#[test]
fn server_bootstrap_isolated_verification_refuses_user_codeterm_directories() {
    let home = tempfile::tempdir().unwrap();
    let user_data = home.path().join(".codeterm-ci-copy");
    let user_config = home.path().join(".codeterm-dev").join("config");
    std::fs::create_dir_all(&user_data).unwrap();
    std::fs::create_dir_all(&user_config).unwrap();
    let external = tempfile::tempdir().unwrap();
    let bind = "127.0.0.1:18765".parse().unwrap();

    assert!(
        super::validate_isolated_verification(&user_data, external.path(), bind, home.path())
            .is_err()
    );
    assert!(super::validate_isolated_verification(
        external.path(),
        &user_config,
        bind,
        home.path()
    )
    .is_err());

    let new_data = home.path().join(".domios-ci-copy");
    let new_config = home.path().join(".domios-dev").join("config");
    std::fs::create_dir_all(&new_data).unwrap();
    std::fs::create_dir_all(&new_config).unwrap();
    assert!(
        super::validate_isolated_verification(&new_data, external.path(), bind, home.path())
            .is_err()
    );
    assert!(
        super::validate_isolated_verification(external.path(), &new_config, bind, home.path())
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn server_bootstrap_isolated_verification_refuses_symlinked_user_codeterm_directory() {
    let home = tempfile::tempdir().unwrap();
    let user_data = home.path().join(".codeterm");
    std::fs::create_dir(&user_data).unwrap();
    let alias = home.path().join("isolated-alias");
    std::os::unix::fs::symlink(&user_data, &alias).unwrap();
    let external = tempfile::tempdir().unwrap();

    assert!(super::validate_isolated_verification(
        &alias,
        external.path(),
        "127.0.0.1:18765".parse().unwrap(),
        home.path()
    )
    .is_err());
}

#[cfg(unix)]
#[test]
fn server_bootstrap_isolated_verification_refuses_config_file_alias_into_user_data() {
    let home = tempfile::tempdir().unwrap();
    let user_config = home.path().join(".codeterm");
    std::fs::create_dir(&user_config).unwrap();
    let user_config_file = user_config.join("config.toml");
    std::fs::write(&user_config_file, b"[api]\ntoken = 'production'\n").unwrap();
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(&user_config_file, config.path().join("config.toml")).unwrap();

    assert!(super::validate_isolated_verification(
        data.path(),
        config.path(),
        "127.0.0.1:18765".parse().unwrap(),
        home.path()
    )
    .is_err());
}

#[test]
fn server_bootstrap_isolated_verification_refuses_relative_paths_and_non_loopback_binds() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    assert!(super::validate_isolated_verification(
        PathBuf::from("relative-data").as_path(),
        config.path(),
        "127.0.0.1:18765".parse().unwrap(),
        home.path()
    )
    .is_err());
    assert!(super::validate_isolated_verification(
        data.path(),
        config.path(),
        "0.0.0.0:18765".parse().unwrap(),
        home.path()
    )
    .is_err());
    for port in [0, 7685, 7686] {
        assert!(super::validate_isolated_verification(
            data.path(),
            config.path(),
            SocketAddr::from(([127, 0, 0, 1], port)),
            home.path()
        )
        .is_err());
    }
}

#[test]
fn server_bootstrap_production_and_development_validation_stays_strict() {
    let resources = PathBuf::from("resources");
    let production = LaunchOptions {
        instance: InstanceSelector::Production,
        resources: resources.clone(),
        probe_sink: None,
        probe_identity_alias: None,
        development_owner: None,
    };
    let development = LaunchOptions {
        instance: InstanceSelector::Development {
            instance: Some("fixture".into()),
            port: 18765,
        },
        resources,
        probe_sink: None,
        probe_identity_alias: None,
        development_owner: None,
    };

    assert!(production.validate(false).is_ok());
    assert!(production.validate(true).is_err());
    assert!(development.validate(true).is_ok());
    assert!(development.validate(false).is_err());
}

#[test]
fn same_machine_clients_dial_loopback_for_any_local_bind() {
    let ready = |bind: &str| super::ServerReady {
        protocol: 2,
        api_version: 3,
        build: "fixture".into(),
        instance_id: "fixture".into(),
        identity: super::ServerIdentity {
            data: PathBuf::from("data"),
            config: PathBuf::from("config"),
        },
        epoch: "fixture".into(),
        process: super::ProcessProvenance {
            pid: 1,
            created: "1".into(),
            executable: PathBuf::from("server"),
        },
        api_address: bind.parse().unwrap(),
        socket_address: String::new(),
        bootstrap_address: String::new(),
        resources: PathBuf::from("resources"),
    };
    assert_eq!(
        ready("0.0.0.0:7685").loopback_api_address(),
        "127.0.0.1:7685".parse().ok()
    );
    assert_eq!(
        ready("[::]:7685").loopback_api_address(),
        "[::1]:7685".parse().ok()
    );
    assert_eq!(
        ready("127.0.0.1:7686").loopback_api_address(),
        "127.0.0.1:7686".parse().ok()
    );
    assert_eq!(ready("192.168.1.10:7685").loopback_api_address(), None);
}

#[test]
fn server_bootstrap_debug_verification_instance_survives_wire_and_rejects_invalid_identity() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let mut options = isolated_options(data.path().into(), config.path().into(), "127.0.0.1:7798".parse().unwrap());
    for invalid in ["", "Upper", "../other", "with_space", &"a".repeat(65)] {
        if let InstanceSelector::IsolatedVerification { instance, .. } = &mut options.instance {
            *instance = Some(invalid.into());
        }
        let decoded: LaunchOptions = serde_json::from_value(serde_json::to_value(&options).unwrap()).unwrap();
        assert!(decoded.validate(true).is_err());
        assert!(decoded.validate(false).is_err());
    }
    if let InstanceSelector::IsolatedVerification { instance, .. } = &mut options.instance {
        *instance = Some("verification-a".into());
    }
    let encoded = serde_json::to_value(&options).unwrap();
    let decoded: LaunchOptions = serde_json::from_value(encoded.clone()).unwrap();
    assert!(decoded.validate(true).is_ok());
    assert!(decoded.validate(false).is_err());
    assert_eq!(encoded["instance"]["instance"], "verification-a");
    assert_eq!(encoded["instance"]["data_dir"], serde_json::json!(data.path()));
    assert_eq!(encoded["instance"]["config_dir"], serde_json::json!(config.path()));
    let mut missing = encoded.clone();
    missing["instance"].as_object_mut().unwrap().remove("instance");
    let decoded: LaunchOptions = serde_json::from_value(missing).unwrap();
    assert!(decoded.validate(true).is_err());
    assert!(decoded.validate(false).is_ok());
    let mut unknown = encoded;
    unknown["instance"]["socket_override"] = serde_json::json!("foreign.sock");
    assert!(serde_json::from_value::<LaunchOptions>(unknown).is_err());
}
