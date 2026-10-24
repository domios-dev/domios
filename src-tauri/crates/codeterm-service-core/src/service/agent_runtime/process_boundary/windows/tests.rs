use super::*;
use std::{io::Write, sync::Arc, time::Duration};
use windows_sys::Win32::System::JobObjects::*;

mod fixture;

const HELPER: &str =
    "service::agent_runtime::process_boundary::windows::tests::external_conpty_peer";

#[test]
#[ignore = "Subprocess fixture invoked by external ConPTY cleanup tests"]
fn external_conpty_peer() {
    if std::env::var_os("DOMIOS_EXTERNAL_TREE_FIXTURE").is_some() {
        fixture::launch();
    }
}

struct Tree {
    root: tokio::process::Child,
    boundary: Arc<Boundary>,
    witnesses: Vec<Member>,
}

impl Drop for Tree {
    fn drop(&mut self) {
        for member in &self.witnesses {
            let _ = member.terminate_retained();
        }
        let _ = self.boundary.job.try_terminate();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        for member in &self.witnesses {
            let _ = member.wait(deadline);
        }
    }
}

fn outside(job: &crate::service::windows_job::Job, member: &Member) -> bool {
    !unsafe { job.contains_process(member.handle as HANDLE) }.unwrap()
}

async fn external_tree() -> Tree {
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", HELPER, "--ignored", "--nocapture"])
        .env("DOMIOS_EXTERNAL_TREE_FIXTURE", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true);
    crate::process_util::no_window_tokio_suspended(&mut command);
    let mut root = command.spawn().unwrap();
    let job = Arc::new(crate::service::windows_job::Job::contain(&root).unwrap());
    let boundary = Arc::new(Boundary::new(root.id().unwrap(), job.clone()).unwrap());
    crate::service::windows_job::resume(&root).unwrap();
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut lines = BufReader::new(root.stdout.take().unwrap()).lines();
    let value: serde_json::Value = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let line = lines
                .next_line()
                .await
                .unwrap()
                .expect("fixture exited before readiness");
            if let Some((_, json)) = line.split_once("fixture-pids:") {
                break serde_json::from_str(json).unwrap();
            }
        }
    })
    .await
    .unwrap();
    let commands: Vec<u32> = serde_json::from_value(value["commands"].clone()).unwrap();
    assert_eq!(commands.len(), 2);
    let shell = Member::retain(value["shell"].as_u64().unwrap() as u32)
        .unwrap()
        .unwrap();
    let witnesses = commands
        .iter()
        .map(|pid| Member::retain(*pid).unwrap().unwrap())
        .collect();
    let tree = Tree {
        root,
        boundary,
        witnesses,
    };
    assert!(
        outside(&job, &shell),
        "shell must start outside runtime Job"
    );
    for witness in &tree.witnesses {
        assert!(
            outside(&job, witness),
            "command must be outside runtime Job"
        );
        assert!(!witness.exited().unwrap());
    }
    tree
}

#[tokio::test]
async fn runtime_job_alone_leaves_external_conpty_commands_alive() {
    let mut tree = external_tree().await;
    tree.boundary.job.try_terminate().unwrap();
    tree.root.wait().await.unwrap();
    for witness in &tree.witnesses {
        assert!(
            !witness.exited().unwrap(),
            "Job-only control must leave commands alive"
        );
    }
}

#[tokio::test]
async fn retirement_captures_external_conpty_descendants() {
    let mut tree = external_tree().await;
    let boundary = tree.boundary.clone();
    tokio::task::spawn_blocking(move || boundary.retire())
        .await
        .unwrap()
        .unwrap();
    for witness in &tree.witnesses {
        assert!(
            witness.exited().unwrap(),
            "external command survived retirement"
        );
    }
    tree.root.wait().await.unwrap();
}

#[test]
fn retained_survivor_reports_its_exit_deadline() {
    let pid = std::process::id();
    let witness = Member::retain(pid).unwrap().unwrap();
    let error = witness.wait(std::time::Instant::now()).unwrap_err();
    assert!(error.contains(&format!("pid={pid}:")));
    assert!(error.contains("deadline"));
    assert!(!witness.exited().unwrap());
}
