use super::*;
use clap::Parser;

fn parse(args: &[&str]) -> MemEmbedderAction {
    let cli = crate::Cli::try_parse_from(args).unwrap();
    let crate::Commands::Mem {
        action: crate::MemAction::Embedder { action },
    } = cli.command
    else {
        panic!("not a mem embedder command");
    };
    action
}

#[test]
fn mem_embedder_verbs_parse_and_map_to_their_daemon_methods() {
    assert_eq!(
        method_and_params(&parse(&["domios", "mem", "embedder", "list"])).0,
        "mem.embedder.list"
    );
    assert_eq!(
        method_and_params(&parse(&["domios", "mem", "embedder", "status"])).0,
        "mem.embedder.status"
    );
    let action = parse(&[
        "domios",
        "mem",
        "embedder",
        "use",
        "openai",
        "--model",
        "text-embedding-3-small",
        "--dimension",
        "256",
    ]);
    let (method, params) = method_and_params(&action);
    assert_eq!(method, "mem.embedder.use");
    assert_eq!(
        params.unwrap(),
        serde_json::json!({ "provider": "openai", "model": "text-embedding-3-small", "dimension": 256 })
    );
    let rendered = render_use(&serde_json::json!({
        "target": { "provider": "local", "model": "m", "dimension": 384 },
        "plan": { "items": 3, "entities": 2, "docs": 1 }, "total": 6, "warning": null
    }));
    assert!(rendered.contains("Re-embedding 6 items"));
}

#[test]
fn mem_embedder_key_set_parses_without_an_argv_key_and_stays_local() {
    let action = parse(&["domios", "mem", "embedder", "key", "set", "openai"]);
    assert!(
        matches!(action, MemEmbedderAction::Key { action: crate::MemEmbedderKeyAction::Set { provider } } if provider == "openai")
    );
    assert!(crate::Cli::try_parse_from([
        "domios", "mem", "embedder", "key", "set", "openai", "fake-key"
    ])
    .is_err());
    let cli =
        crate::Cli::try_parse_from(["domios", "mem", "embedder", "key", "set", "openai"]).unwrap();
    assert_eq!(
        crate::transport::transport_for(&cli.command).local,
        crate::transport::Local::PreSocket
    );
    assert!(matches!(
        crate::transport::transport_for(&cli.command).api_verdict,
        crate::transport::ApiVerdict::Unsupported(_)
    ));
}
