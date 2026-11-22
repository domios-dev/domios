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
    shell: Member,
    keeper_shell: Option<Member>,
    witnesses: Vec<Member>,
    roster: Vec<codeterm_plugin::agent_runtime::RuntimeBackgroundProcess>,
}

impl Drop for Tree {
    fn drop(&mut self) {
        for member in &self.witnesses {
            let _ = member.terminate_retained();
        }
        let _ = self.shell.terminate_retained();
        if let Some(keeper) = &self.keeper_shell {
            let _ = keeper.terminate_retained();
        }
        let _ = self.boundary.job.try_terminate();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let _ = self.shell.wait(deadline);
        if let Some(keeper) = &self.keeper_shell {
            let _ = keeper.wait(deadline);
        }
        for member in &self.witnesses {
            let _ = member.wait(deadline);
        }
    }
}

fn outside(job: &crate::service::windows_job::Job, member: &Member) -> bool {
    !unsafe { job.contains_process(member.handle as HANDLE) }.unwrap()
}

async fn external_tree() -> Tree {
    fixture_tree(false).await
}

async fn fixture_tree(selective: bool) -> Tree {
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", HELPER, "--ignored", "--nocapture"])
        .env("DOMIOS_EXTERNAL_TREE_FIXTURE", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true);
    if selective {
        command.env("DOMIOS_SELECTIVE_TREE_FIXTURE", "1");
    }
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
    let terminals = value["terminals"].as_array().unwrap();
    assert_eq!(terminals.len(), if selective { 2 } else { 1 });
    let cwd = value["cwd"].as_str().unwrap();
    let mut roster = Vec::new();
    let mut witnesses = Vec::new();
    let mut shells = Vec::new();
    for (index, terminal) in terminals.iter().enumerate() {
        let commands: Vec<u32> = serde_json::from_value(terminal["commands"].clone()).unwrap();
        assert_eq!(commands.len(), 2);
        let shell = Member::retain(terminal["shell"].as_u64().unwrap() as u32)
            .unwrap()
            .unwrap();
        assert!(
            outside(&job, &shell),
            "shell must start outside runtime Job"
        );
        shells.push(shell);
        witnesses.extend(
            commands
                .into_iter()
                .map(|pid| Member::retain(pid).unwrap().unwrap()),
        );
        roster.push(codeterm_plugin::agent_runtime::RuntimeBackgroundProcess {
            process_id: format!("background-{index}"),
            item_id: format!("item-{index}"),
            command: terminal["command"].as_str().unwrap().into(),
            cwd: cwd.into(),
            os_pid: None,
        });
    }
    let mut shells = shells.into_iter();
    let tree = Tree {
        root,
        boundary,
        shell: shells.next().unwrap(),
        keeper_shell: shells.next(),
        witnesses,
        roster,
    };
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
#[ignore = "requires the Microsoft Store pwsh app-execution alias; run with --ignored on a Windows host that has it"]
async fn background_stop_reaps_only_the_selected_external_command() {
    let tree = fixture_tree(true).await;
    let boundary = tree.boundary.clone();
    let roster = tree.roster.clone();
    let captured =
        tokio::task::spawn_blocking(move || boundary.capture_background(&roster[0], &roster[1..]))
            .await
            .unwrap()
            .unwrap()
            .expect("identify the exact target shell with no OS PID");

    tree.shell.terminate_retained().unwrap();
    tree.shell
        .wait(Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert!(tree.witnesses[..2]
        .iter()
        .all(|member| !member.exited().unwrap()));

    let boundary = tree.boundary.clone();
    tokio::task::spawn_blocking(move || boundary.reap_background(captured))
        .await
        .unwrap()
        .unwrap();
    assert!(tree.witnesses[..2]
        .iter()
        .all(|member| member.exited().unwrap()));
    assert!(tree.witnesses[2..]
        .iter()
        .all(|member| !member.exited().unwrap()));
    assert!(!tree.keeper_shell.as_ref().unwrap().exited().unwrap());
}

#[tokio::test]
#[ignore = "requires the Microsoft Store pwsh app-execution alias; run with --ignored on a Windows host that has it"]
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
#[ignore = "requires the Microsoft Store pwsh app-execution alias; run with --ignored on a Windows host that has it"]
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

#[tokio::test]
#[ignore = "requires the Microsoft Store pwsh app-execution alias; run with --ignored on a Windows host that has it"]
async fn stop_reaps_commands_orphaned_outside_every_job() {
    let tree = external_tree().await;
    let boundary = tree.boundary.clone();
    let tools = tokio::task::spawn_blocking(move || boundary.capture_tools())
        .await
        .unwrap()
        .unwrap();
    let tools = Arc::new(tools);
    let owner = tree.boundary.clone();
    let reap = |tools: Arc<super::super::ToolTree>| {
        let boundary = owner.clone();
        tokio::task::spawn_blocking(move || boundary.reap_orphans(&tools))
    };
    assert!(
        reap(tools.clone()).await.unwrap().unwrap().is_empty(),
        "commands under a live shell are not orphans"
    );
    // Only the shell ends, as a provider ends an interrupted tool; the fixture's ConPTY stays open,
    // since closing it would end the console-attached commands by itself.
    tree.shell.terminate_retained().unwrap();
    tree.shell
        .wait(std::time::Instant::now() + Duration::from_secs(10))
        .unwrap();
    let orphans: Vec<&Member> = tree
        .witnesses
        .iter()
        .filter(|witness| outside(&tree.boundary.job, witness) && !witness.exited().unwrap())
        .collect();
    assert!(
        !orphans.is_empty(),
        "shell teardown must leave an out-of-Job command alive to prove reaping"
    );
    let reaped = reap(tools).await.unwrap().unwrap();
    for orphan in orphans {
        let pid = unsafe { GetProcessId(orphan.handle as HANDLE) };
        assert!(reaped.contains(&pid), "orphan {pid} was not reaped");
        assert!(orphan.exited().unwrap(), "orphan {pid} survived Stop");
    }
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
