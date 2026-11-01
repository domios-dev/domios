use super::*;
use interprocess::local_socket::{
    prelude::*, GenericNamespaced, ListenerNonblockingMode, ListenerOptions,
};
use std::io::{BufRead, BufReader, Read, Write};
use std::time::Instant;

#[test]
fn local_resolution_sends_first_request_on_one_connection() {
    let _guard = test_env_lock().lock().unwrap();
    let name = format!("thin-{}-{}", std::process::id(), uuid::Uuid::new_v4());
    let listener = ListenerOptions::new()
        .name(name.as_str().to_ns_name::<GenericNamespaced>().unwrap())
        .create_sync()
        .unwrap();
    let keys = [
        "CODETERM_SOCKET_PATH",
        "CODETERM_INSTANCE",
        "CODETERM_API_PORT",
        "CODETERM_API_TOKEN",
        "CODETERM_TAB_ID",
    ];
    let previous = keys.map(|key| std::env::var(key).ok());
    std::env::set_var(keys[0], &name);
    std::env::remove_var(keys[1]);
    std::env::remove_var(keys[2]);
    std::env::set_var(keys[3], "fixture-launch-token");
    std::env::set_var(keys[4], "fixture-tab");
    let start = Instant::now();
    let server = std::thread::spawn(move || {
        let mut accepted = 0;
        let elapsed = loop {
            let mut stream = BufReader::new(listener.accept().unwrap());
            accepted += 1;
            let mut first = [0];
            if stream.read_exact(&mut first).is_err() {
                continue;
            }
            let elapsed = start.elapsed();
            let mut line = String::new();
            stream.read_line(&mut line).unwrap();
            line.insert(0, first[0] as char);
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "system.identify");
            assert_eq!(request["credential"], "fixture-launch-token");
            writeln!(
                stream.get_mut(),
                "{}",
                serde_json::json!({"jsonrpc":"2.0","id":request["id"],"result":{}})
            )
            .unwrap();
            break elapsed;
        };
        (listener, accepted, elapsed)
    });
    let (mut client, _) = SocketClient::connect_local().unwrap();
    client.call("system.identify", None).unwrap();
    let (listener, accepted, elapsed) = server.join().unwrap();
    for (key, value) in keys.into_iter().zip(previous) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
    listener
        .set_nonblocking(ListenerNonblockingMode::Accept)
        .unwrap();
    assert_eq!(accepted, 1);
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    println!("resolution to first request: {elapsed:?}");
    if !cfg!(debug_assertions) {
        assert!(elapsed < std::time::Duration::from_millis(25));
    }
}

struct BindingClient;
impl client::RpcClient for BindingClient {
    fn call_detailed(
        &mut self,
        method: &str,
        _: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, client::SocketCallError> {
        assert_eq!(method, "plan.binding.resolve");
        Ok(serde_json::json!({"binding":{"plan_id":"plan_bound","task_id":"task_bound"}}))
    }
}

#[test]
fn binding_consumers_reparse_hydrated_defaults() {
    let _guard = test_env_lock().lock().unwrap();
    let keys = ["CODETERM_TAB_ID", "CODETERM_PLAN_ID", "CODETERM_TASK_ID"];
    let previous = keys.map(|key| std::env::var(key).ok());
    std::env::set_var(keys[0], "tab_bound");
    for args in [
        vec!["agent", "ack"],
        vec!["agent", "report"],
        vec!["plan", "record"],
        vec!["orchestrator", "status"],
    ] {
        std::env::remove_var(keys[1]);
        std::env::remove_var(keys[2]);
        let args: Vec<String> = std::iter::once("codeterm")
            .chain(args)
            .map(String::from)
            .collect();
        let parsed = Cli::try_parse_from(&args).unwrap();
        assert!(command_consumes_binding(&parsed.command));
        hydrate_tab_task_binding_env(&args, &mut BindingClient);
        let parsed = Cli::try_parse_from(&args).unwrap();
        match parsed.command {
            Commands::Agent {
                action: AgentAction::Ack { plan, task, .. },
            } => {
                assert_eq!(plan.as_deref(), Some("plan_bound"));
                assert_eq!(task.as_deref(), Some("task_bound"));
            }
            Commands::Agent {
                action: AgentAction::Report(report),
            } => {
                assert_eq!(report.plan.as_deref(), Some("plan_bound"));
                assert_eq!(report.task.as_deref(), Some("task_bound"));
            }
            Commands::Plan {
                action: PlanAction::Record { plan_id, task, .. },
            } => {
                assert_eq!(plan_id.as_deref(), Some("plan_bound"));
                assert_eq!(task.as_deref(), Some("task_bound"));
            }
            Commands::Orchestrator { .. } => assert_eq!(
                nonempty_env("CODETERM_PLAN_ID").as_deref(),
                Some("plan_bound")
            ),
            _ => panic!("unexpected binding consumer"),
        }
    }
    assert!(!command_consumes_binding(
        &Cli::try_parse_from(["codeterm", "identify"])
            .unwrap()
            .command
    ));
    for (key, value) in keys.into_iter().zip(previous) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
