use super::*;
use crate::agent_runtime::{validate_runtime_operation, RuntimeOperation, RuntimeThreadParent};

fn context() -> RuntimeContext {
    RuntimeContext {
        restricted_cli: None,
        host_owned: Default::default(),
        epoch: 1,
        tab_id: "tab".into(),
        binding_nonce: "nonce".into(),
        cwd: "/work".into(),
        host_version: "1".into(),
        thread_id: Some("root".into()),
        thread_parents: vec![RuntimeThreadParent {
            thread_id: "child".into(),
            parent_thread_id: "root".into(),
        }],
        ..Default::default()
    }
}

#[test]
fn only_proven_root_and_descendants_are_owned() {
    let mut ctx = context();
    assert!(owns_runtime_thread(&ctx, "root"));
    assert!(owns_runtime_thread(&ctx, "child"));
    let op = RuntimeOperation::BackgroundProcesses {
        thread_id: "foreign".into(),
        action: RuntimeBackgroundProcessAction::Clean,
    };
    assert!(validate_runtime_operation(&ctx, &op).is_err());
    ctx.thread_parents[0].parent_thread_id = "child".into();
    assert!(!owns_runtime_thread(&ctx, "child"));
}

#[test]
fn rosters_reject_duplicates_and_oversize_values() {
    let process = RuntimeBackgroundProcess {
        process_id: "pid".into(),
        item_id: "item".into(),
        command: "command".into(),
        cwd: "/work".into(),
        os_pid: Some(42),
    };
    assert!(validate_background_processes(std::slice::from_ref(&process)).is_ok());
    assert!(validate_background_processes(&[process.clone(), process.clone()]).is_err());
    let mut large = process;
    large.command = "x".repeat(65537);
    assert!(validate_background_processes(&[large]).is_err());
}
