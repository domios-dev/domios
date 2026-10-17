use super::peer_relay_envelope;
use crate::{Cli, Commands};
use clap::Parser;
use codeterm_mesh::types::RelayCommand;

#[test]
fn mesh_exec_preserves_regex_and_windows_paths_through_json_hops() {
    let command = r#"$pattern='(?ms)^\[api\]\r?\n.*'; $paths=@('.\build.ps1','C:\new\test','\','\\host\share'); Write-Output $pattern"#;
    let cli = Cli::try_parse_from([
        "codeterm", "tab", "exec", "--mesh", "ROG", "--tab", "target", command,
    ])
    .unwrap();
    assert!(matches!(&cli.command, Commands::Tab { .. }));
    let (method, path, body) = crate::mesh_cli::api_map::cli_command_to_api(&cli.command);
    let envelope = peer_relay_envelope("ROG", &method, &path, body.as_deref());
    let request = reqwest::blocking::Client::new()
        .post("http://127.0.0.1/api/mesh/relay-to")
        .json(&envelope)
        .build()
        .unwrap();
    let origin: serde_json::Value =
        serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
    let relay = RelayCommand {
        method: origin["method"].as_str().unwrap().into(),
        path: origin["path"].as_str().unwrap().into(),
        body: origin["body"].clone(),
        delivery_id: None,
    };
    let peer: RelayCommand = serde_json::from_slice(&serde_json::to_vec(&relay).unwrap()).unwrap();
    let forwarded = reqwest::blocking::Client::new()
        .post(format!("http://127.0.0.1{}", peer.path))
        .json(&peer.body)
        .build()
        .unwrap();
    let final_body: serde_json::Value =
        serde_json::from_slice(forwarded.body().unwrap().as_bytes().unwrap()).unwrap();
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/tabs/target/exec");
    assert_eq!(
        final_body["command"].as_str().unwrap().as_bytes(),
        command.as_bytes()
    );
}
