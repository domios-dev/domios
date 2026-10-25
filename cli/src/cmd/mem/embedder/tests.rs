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

#[test]
fn recall_reembed_same_model_receipt_and_failed_status_are_explicit() {
    let receipt = render_use(
        &serde_json::json!({"already_active":true,"target":{"provider":"local","model":"L12"},"total":0}),
    );
    assert!(receipt.contains("already active"));
    assert!(!receipt.contains("Re-embedding"));
    let cancel = render_use(
        &serde_json::json!({"already_active":true,"target":{"provider":"local","model":"L12"},"cancelled":{"provider":"remote","model":"wide"}}),
    );
    assert!(cancel.contains("Cancelled the re-embed to remote/wide"));
    for state in ["failed", "cancelled"] {
        let status = render_status(
            &serde_json::json!({"reembed":{"state":state,"done":94,"total":901,"last_error":"worker dropped"}}),
        );
        assert!(status.contains(state));
        assert!(status.contains("worker dropped"));
        assert!(status.contains("94/901"));
    }
}

#[test]
fn recall_priority_status_prints_live_queue_separately_from_pending() {
    let status = render_status(&serde_json::json!({
        "pending": 0,
        "queue": {"queued": 410, "in_flight": 1, "failed": 2, "unavailable": true}
    }));
    for field in [
        "pending    0",
        "queued     410 chunks",
        "in flight  1",
        "failed     2",
        "queue      unavailable",
    ] {
        assert!(status.contains(field), "missing {field}: {status}");
    }
}

#[test]
fn shadow_residency_status_names_published_target_and_explicit_recovery() {
    let status = render_status(&serde_json::json!({
        "provider":"local", "model":"published", "dimension":384,
        "reembed":{"state":"failed","target":{"provider":"local","model":"target","dimension":768}, "last_error":"outage"}
    }));
    for field in ["model      published", "target     local/target",
        "retry checkpointed work with `domios mem embedder use local --model target --dimension 768`",
        "discard shadow work with `domios mem embedder use local --model published --dimension 384`"] {
        assert!(status.contains(field), "missing {field}: {status}");
    }
}

#[test]
fn docs_reembed_running_receipt_preserves_progress_without_announcing_a_new_run() {
    assert_eq!(
        render_use(&serde_json::json!({
            "already_active": false,
            "already_running": {"done": 5, "total": 816},
            "target": {"provider": "local", "model": "target"}
        })),
        "Re-embed already running (5/816)\n"
    );
}
