use clap::Parser;

#[test]
fn acp_commands_parse_repeated_arguments_and_secret_names() {
    for args in [
        vec!["agent", "acp", "list"],
        vec!["agent", "acp", "remove", "fixture"],
        vec!["agent", "acp", "registry", "search", "fixture"],
        vec!["agent", "acp", "registry", "install", "fixture"],
        vec![
            "agent",
            "acp",
            "add",
            "fixture",
            "--binary",
            "fixture-agent",
            "--arg",
            "--stdio",
            "--arg",
            "second",
            "--env",
            "MODE=fixture",
            "--secret-env",
            "TOKEN=domios:provider:acp:fixture",
            "--auth-env",
            "KEY=domios:provider:acp:key:api-key",
        ],
        vec![
            "agent",
            "acp",
            "add",
            "derived",
            "--binary",
            "fixture-agent",
            "--like",
            "fixture-base",
            "--client-capability",
            "fs.readTextFile=false",
            "--client-capability",
            "terminal=true",
            "--limits-source",
            "unavailable:Quota has no public endpoint",
        ],
        vec![
            "agent",
            "acp",
            "registry",
            "install",
            "fixture",
            "--limits-source",
            "account:fixture-native",
        ],
    ] {
        assert!(
            crate::args::cli::Cli::try_parse_from(std::iter::once("domios").chain(args)).is_ok()
        );
    }
    assert!(
        crate::args::cli::Cli::try_parse_from(["domios", "agent", "acp", "add", "fixture"])
            .is_err()
    );
    assert!(crate::args::cli::Cli::try_parse_from([
        "domios", "agent", "acp", "registry", "install"
    ])
    .is_err());
}

#[test]
fn acp_management_has_no_mesh_transport() {
    let cli = crate::args::cli::Cli::try_parse_from(["domios", "agent", "acp", "list"]).unwrap();
    assert!(!crate::transport::is_socket_relayed(&cli.command));
    assert!(matches!(
        crate::transport::transport_for(&cli.command).api_verdict,
        crate::transport::ApiVerdict::Unsupported(_)
    ));
}
