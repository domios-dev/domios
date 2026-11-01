use super::*;
use clap::Parser;

#[test]
fn backend_delegate_argv_and_local_transport() {
    for to in ["postgres", "sqlite"] {
        for into_nonempty in [false, true] {
            let mut input = vec!["domios", "--json", "mem", "backend", "migrate", "--to", to];
            if into_nonempty {
                input.push("--into-nonempty");
            }
            let cli = crate::Cli::try_parse_from(input).unwrap();
            assert_eq!(
                crate::transport::transport_for(&cli.command).local,
                crate::transport::Local::PreSocket
            );
            assert!(matches!(
                crate::transport::transport_for(&cli.command).api_verdict,
                crate::transport::ApiVerdict::Unsupported(_)
            ));
            let crate::Commands::Mem {
                action: crate::MemAction::Backend { action },
            } = cli.command
            else {
                panic!("wrong command")
            };
            let mut expected = vec!["memory-migrate", "--to", to];
            if into_nonempty {
                expected.push("--into-nonempty");
            }
            expected.push("--json");
            assert_eq!(argv(&action, true), expected);
            expected.pop();
            assert_eq!(argv(&action, false), expected);
        }
    }
    for tail in [vec![], vec!["--to", ""], vec!["--to", "other"]] {
        let mut input = vec!["domios", "mem", "backend", "migrate"];
        input.extend(tail);
        assert!(crate::Cli::try_parse_from(input).is_err());
    }
}

#[test]
fn backend_delegate_refuses_missing_packaged_server() {
    let dir = tempfile::tempdir().unwrap();
    let cli = dir.path().join("cli");
    std::fs::create_dir(&cli).unwrap();
    let executable = cli.join("domios");
    std::fs::write(&executable, b"fixture").unwrap();
    assert!(server(&executable)
        .unwrap_err()
        .to_string()
        .contains("reinstall the CLI"));
}

#[test]
fn backend_delegate_refuses_tampered_resource_hint_and_stale_digest() {
    use codeterm_types::server_bootstrap::ClientIdentity;
    let dir = tempfile::tempdir().unwrap();
    let installed = dir.path().join("bin");
    let resources = dir.path().join("resources");
    std::fs::create_dir_all(&installed).unwrap();
    std::fs::create_dir_all(resources.join("cli")).unwrap();
    std::fs::create_dir_all(resources.join("domios-server")).unwrap();
    let executable = installed.join("domios");
    std::fs::write(&executable, b"cli").unwrap();
    std::fs::write(
        resources.join(format!("cli/codeterm{}", std::env::consts::EXE_SUFFIX)),
        b"cli",
    )
    .unwrap();
    let relative = std::path::PathBuf::from(format!(
        "domios-server/domios-server{}",
        std::env::consts::EXE_SUFFIX
    ));
    let server_path = resources.join(&relative);
    std::fs::write(&server_path, b"server").unwrap();
    let manifest = ServerComponentManifest {
        schema: SERVER_COMPONENT_SCHEMA,
        server_build: env!("CODETERM_CLI_BUILD_SHA").into(),
        server_executable: relative,
        server_sha256: digest(&server_path).unwrap(),
        client_identity: ClientIdentity::None,
        api_version: API_VERSION,
        bootstrap_protocol: BOOTSTRAP_PROTOCOL,
    };
    std::fs::write(
        resources.join("server-component.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let mut hint = CliResourceHint {
        resources: resources.clone(),
        cli_sha256: digest(&executable).unwrap(),
    };
    let hint_path = installed.join(CLI_RESOURCE_HINT_FILE);
    std::fs::write(&hint_path, serde_json::to_vec(&hint).unwrap()).unwrap();
    assert_eq!(
        server(&executable).unwrap(),
        server_path.canonicalize().unwrap()
    );
    std::fs::write(&server_path, b"different server").unwrap();
    assert!(server(&executable).is_err());
    hint.resources = dir.path().join("foreign");
    std::fs::write(&hint_path, serde_json::to_vec(&hint).unwrap()).unwrap();
    assert!(server(&executable).is_err());
    hint.resources = resources;
    hint.cli_sha256 = "a".repeat(64);
    std::fs::write(&hint_path, serde_json::to_vec(&hint).unwrap()).unwrap();
    assert!(server(&executable).is_err());
}

#[test]
fn backend_delegate_pins_the_cli_resolved_data_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("named-instance");
    let server = dir.path().join("verified-server");
    let action = MemBackendAction::Migrate {
        to: "postgres".into(),
        into_nonempty: false,
    };
    let command = delegate(&server, &action, true, &root);
    assert_eq!(command.get_program(), server.as_os_str());
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        vec!["memory-migrate", "--to", "postgres", "--json"]
    );
    assert_eq!(
        command.get_envs().collect::<Vec<_>>(),
        vec![(
            std::ffi::OsStr::new("CODETERM_DATA_DIR"),
            Some(root.as_os_str())
        )]
    );
}
